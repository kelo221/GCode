use crate::app::settings::SettingsSection;
use crate::app::settings_provider_model_dialog::DialogError;
use crate::app::{root::RootView, store::AppState, test_support::*};
use crate::provider_settings::model_draft::DraftField;
use crate::shared::preferences::Preferences;
use crate::shared::settings::AppSettings;
use gpui::{MouseButton, Pixels, Point, TestApp, TestAppWindow, px, size};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

fn app(path: std::path::PathBuf) -> TestApp {
    let mut app = TestApp::with_text_system_and_assets(
        Arc::new(gpui::NoopTextSystem::new()),
        Arc::new(ely_gpui_component::Assets),
    );
    app.update(|cx| {
        ely_gpui_component::init(cx);
        cx.set_global(TestTargets::default());
        Preferences::install_at(AppSettings::default(), path, Arc::new(|| false), cx);
    });
    app
}

fn target(app: &mut TestApp, id: &str) -> Point<Pixels> {
    app.read(|cx| {
        cx.global::<TestTargets>()
            .0
            .get(id)
            .unwrap_or_else(|| panic!("target {id} not rendered"))
            .center()
    })
}

fn sent(rx: &Receiver<String>) -> Vec<Value> {
    rx.try_iter()
        .map(|line| serde_json::from_str(&line).unwrap())
        .collect()
}

fn set_id(window: &mut TestAppWindow<RootView>, text: &str) {
    window.update(|view, _, cx| {
        let input = view.add_model_dialog().unwrap().id.clone();
        input.update(cx, |input, cx| input.set_text(text, cx));
    });
}

fn error(window: &mut TestAppWindow<RootView>) -> Option<DialogError> {
    window.read(|view, _| view.add_model_dialog().and_then(|d| d.error.clone()))
}

fn open(
    app: &mut TestApp,
    state: &gpui::Entity<AppState>,
) -> (TestAppWindow<RootView>, String, Receiver<String>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let key = app.update_entity(state, |s, _| {
        s.workspaces[0].inbound = Some(tx);
        s.workspaces[0].started = true;
        s.active_workspace = Some(s.workspaces[0].key.clone());
        s.provider_settings.view = crate::provider_settings::ProviderView::from_value(
            &crate::provider_settings::tests::sample_view(),
        );
        s.provider_settings.selected = Some("c1".into());
        s.workspaces[0].key.clone()
    });
    let mut window = app.open_window(|_, cx| RootView::new(state.clone(), cx));
    window.simulate_resize(size(px(1280.), px(900.)));
    window
        .update(|view, window, cx| view.open_settings_section(SettingsSection::Models, window, cx));
    let _ = sent(&rx);
    app.update_entity(state, |s, _| s.provider_settings.loading = false);
    window.draw();
    let add = target(app, "provider-model-add");
    window.simulate_click(add, MouseButton::Left);
    window.draw();
    (window, key, rx)
}

#[test]
fn add_model_dialog_validates_resolves_then_saves_and_closes() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("gpui-model-dialog-{}", uuid::Uuid::now_v7()));
    let mut app = app(dir.join("setting.json"));
    let state = app.new_entity(AppState::for_test);
    let (mut window, key, rx) = open(&mut app, &state);
    window.read(|view, _| assert!(view.add_model_dialog().is_some(), "Add opens the dialog"));

    // 空 ID 与重复 ID 都给出行内错误，不再静默无反应。
    let save = target(&mut app, "provider-model-dialog-save");
    window.simulate_click(save, MouseButton::Left);
    assert_eq!(error(&mut window), Some(DialogError::Field(DraftField::Id)));
    set_id(&mut window, " m1 ");
    window.update(|view, _, cx| view.save_add_model_dialog(cx));
    assert_eq!(error(&mut window), Some(DialogError::Duplicate));
    assert!(sent(&rx).is_empty());

    // Smart mode resolves the recommended config first, then saves.
    set_id(&mut window, "glm-new");
    window.update(|view, _, cx| view.save_add_model_dialog(cx));
    let resolve = sent(&rx);
    assert_eq!(resolve.len(), 1);
    assert_eq!(resolve[0]["method"], "providerSettings/resolveModelConfig");
    assert_eq!(
        resolve[0]["params"],
        json!({ "providerId": "c1", "modelId": "glm-new" })
    );
    let inherited = json!({
        "properties": {
            "contextWindow": 128000,
            "inputFormat": { "supportsText": true, "supportsImage": false,
                "supportsVideo": false, "supportsPdf": false },
            "supportsJsonSchemaOutput": false,
            "supportsNativeWebSearch": false,
            "supportsMidConversationSystem": false
        },
        "optionSpecs": { "reasoningLevel": { "values": ["low"], "map": "{}" } }
    });
    app.update_entity(&state, |s, cx| {
        let id = resolve[0]["id"].as_u64().unwrap();
        let result = json!({ "inheritedConfig": inherited, "effectiveConfig": {}, "issues": [] });
        s.handle_response(&key, id, Some(result), None, cx);
    });
    let add = sent(&rx);
    assert_eq!(add.len(), 1, "save continues after the resolution");
    assert_eq!(
        add[0]["params"]["operation"],
        json!({
            "type": "addModel",
            "providerId": "c1",
            "modelId": "glm-new",
            "config": {},
            "useRecommendedConfig": true,
        })
    );
    window.read(|view, _| assert!(view.add_model_dialog().unwrap().saving()));

    // An agent error stays in the dialog; the page error is untouched.
    app.update_entity(&state, |s, cx| {
        let id = add[0]["id"].as_u64().unwrap();
        let error = json!({ "code": -32602, "message": "map does not compile" });
        s.handle_response(&key, id, None, Some(error), cx);
    });
    assert_eq!(
        error(&mut window),
        Some(DialogError::Backend("map does not compile".into()))
    );
    assert!(app.read_entity(&state, |s, _| s.provider_settings.error.is_none()));

    window.update(|view, _, cx| view.save_add_model_dialog(cx));
    let retry = sent(&rx);
    assert_eq!(retry.len(), 1, "the cached resolution is reused");
    app.update_entity(&state, |s, cx| {
        let id = retry[0]["id"].as_u64().unwrap();
        let view = crate::provider_settings::tests::sample_view();
        s.handle_response(&key, id, Some(json!({ "view": view })), None, cx);
    });
    window.read(|view, _| assert!(view.add_model_dialog().is_none(), "closes on success"));
    assert!(!dir.exists());
}

#[test]
fn manual_mode_requires_limits_and_escape_cancels() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("gpui-model-manual-{}", uuid::Uuid::now_v7()));
    let mut app = app(dir.join("setting.json"));
    let state = app.new_entity(AppState::for_test);
    let (mut window, _key, rx) = open(&mut app, &state);
    set_id(&mut window, "custom");
    window.update(|view, _, cx| view.set_model_dialog_recommended(false, cx));
    window.update(|view, _, cx| view.save_add_model_dialog(cx));
    assert_eq!(
        error(&mut window),
        Some(DialogError::Field(DraftField::ContextWindow))
    );
    window.update(|view, _, cx| {
        let dialog = view.add_model_dialog().unwrap();
        let (context, max) = (dialog.context_window.clone(), dialog.max_output.clone());
        let (level, map) = (dialog.new_level.clone(), dialog.level_map.clone());
        context.update(cx, |i, cx| i.set_text("64000", cx));
        max.update(cx, |i, cx| i.set_text("8000", cx));
        level.update(cx, |i, cx| i.set_text("high", cx));
        map.update(cx, |i, cx| i.set_text("{'effort': reasoningLevel}", cx));
    });
    window.update(|view, _, cx| view.add_model_level(cx));
    window.update(|view, _, cx| view.save_add_model_dialog(cx));
    let ops = sent(&rx);
    assert_eq!(ops.len(), 1, "manual mode never resolves");
    let operation = &ops[0]["params"]["operation"];
    assert_eq!(operation["useRecommendedConfig"], false);
    assert_eq!(operation["config"]["properties"]["contextWindow"], 64000);
    assert_eq!(
        operation["config"]["optionSpecs"],
        json!({
            "reasoningLevel": { "values": ["high"], "map": "{'effort': reasoningLevel}" },
            "maxOutputTokens": { "max": 8000 }
        })
    );

    // A second dialog: Escape cancels without sending anything.
    app.update_entity(&state, |s, _| {
        s.provider_settings.busy = false;
        s.provider_settings.added = None;
    });
    window.update(|view, _, cx| {
        if let Some(form) = view.settings.provider_form.as_mut() {
            form.add_model = None;
        }
        cx.notify();
    });
    window.draw();
    let add = target(&mut app, "provider-model-add");
    window.simulate_click(add, MouseButton::Left);
    window.draw();
    window.draw();
    window.simulate_keystroke("escape");
    window.read(|view, _| assert!(view.add_model_dialog().is_none(), "Escape cancels"));
    assert!(sent(&rx).is_empty());
    assert!(!dir.exists());
}

#[test]
fn settings_button_edits_the_saved_model_against_the_open_revision() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("gpui-model-edit-{}", uuid::Uuid::now_v7()));
    let mut app = app(dir.join("setting.json"));
    let state = app.new_entity(AppState::for_test);
    let (mut window, key, rx) = open(&mut app, &state);
    window.update(|view, _, cx| {
        view.settings.provider_form.as_mut().unwrap().add_model = None;
        cx.notify();
    });
    // m1 carries a saved context-window override on top of a complete baseline.
    let baseline = |context: u64| {
        json!({
            "properties": {
                "contextWindow": context,
                "inputFormat": { "supportsText": true, "supportsImage": true,
                    "supportsVideo": false, "supportsPdf": false },
                "supportsJsonSchemaOutput": false,
                "supportsNativeWebSearch": false,
                "supportsMidConversationSystem": false
            },
            "optionSpecs": { "reasoningLevel": { "values": ["low"], "map": "{}" } }
        })
    };
    app.update_entity(&state, |s, _| {
        let mut view = crate::provider_settings::tests::sample_view();
        let model = &mut view["providers"][0]["models"][0];
        model["effectiveConfig"] = baseline(200000);
        model["personalExactConfig"] = json!({ "properties": { "contextWindow": 200000 } });
        s.provider_settings.view = crate::provider_settings::ProviderView::from_value(&view);
    });
    window.draw();
    let edit = target(&mut app, "provider-model-edit-m1");
    window.simulate_click(edit, MouseButton::Left);
    window.draw();
    window.read(|view, cx| {
        let dialog = view
            .add_model_dialog()
            .expect("the settings button opens the dialog");
        assert_eq!(dialog.editing.as_ref().unwrap().id, "m1");
        assert_eq!(dialog.id.read(cx).text(), "m1");
        assert_eq!(dialog.context_window.read(cx).text(), "200000");
    });
    // 编辑模式的推荐基线只取内置推荐（personalConfig: {}），打开即请求。
    let resolve = sent(&rx);
    assert_eq!(resolve.len(), 1);
    assert_eq!(
        resolve[0]["params"],
        json!({ "providerId": "c1", "modelId": "m1", "personalConfig": {} })
    );
    app.update_entity(&state, |s, cx| {
        let id = resolve[0]["id"].as_u64().unwrap();
        let result =
            json!({ "inheritedConfig": baseline(128000), "effectiveConfig": {}, "issues": [] });
        s.handle_response(&key, id, Some(result), None, cx);
    });

    // Its own ID is not a duplicate; the save targets the revision it opened on.
    window.update(|view, _, cx| view.save_add_model_dialog(cx));
    assert_eq!(error(&mut window), None);
    let ops = sent(&rx);
    assert_eq!(ops.len(), 1);
    assert_eq!(
        ops[0]["params"]["operation"],
        json!({
            "type": "updateModel",
            "providerId": "c1",
            "modelId": "m1",
            "nextModelId": "m1",
            "config": { "properties": { "contextWindow": 200000 } },
            "useRecommendedConfig": true,
            "basedOnRevision": 7,
        })
    );
    assert!(!dir.exists());
}

#[test]
fn id_typing_resolves_after_idle_and_drops_stale_answers() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("gpui-model-idle-{}", uuid::Uuid::now_v7()));
    let mut app = app(dir.join("setting.json"));
    let state = app.new_entity(AppState::for_test);
    let (mut window, key, rx) = open(&mut app, &state);
    set_id(&mut window, "glm-a");
    app.advance_clock(std::time::Duration::from_millis(600));
    app.run_until_parked();
    assert!(sent(&rx).is_empty(), "still typing");
    set_id(&mut window, "glm-b");
    app.advance_clock(std::time::Duration::from_millis(1300));
    app.run_until_parked();
    let first = sent(&rx);
    assert_eq!(first.len(), 1, "one resolve for the settled ID");
    assert_eq!(first[0]["params"]["modelId"], "glm-b");

    // The ID moved on before the answer arrived: the old answer is dropped.
    set_id(&mut window, "glm-c");
    app.update_entity(&state, |s, cx| {
        let id = first[0]["id"].as_u64().unwrap();
        let result = json!({ "inheritedConfig": { "properties": { "contextWindow": 1 } },
            "effectiveConfig": {}, "issues": [] });
        s.handle_response(&key, id, Some(result), None, cx);
    });
    window.read(|view, cx| {
        let dialog = view.add_model_dialog().unwrap();
        assert!(dialog.pending());
        assert_eq!(
            dialog.context_window.read(cx).placeholder_text().as_ref(),
            ""
        );
    });
    app.advance_clock(std::time::Duration::from_millis(1300));
    app.run_until_parked();
    let second = sent(&rx);
    assert_eq!(second[0]["params"]["modelId"], "glm-c");
    app.update_entity(&state, |s, cx| {
        let id = second[0]["id"].as_u64().unwrap();
        let result = json!({ "inheritedConfig": { "properties": { "contextWindow": 64000 } },
            "effectiveConfig": {}, "issues": [] });
        s.handle_response(&key, id, Some(result), None, cx);
    });
    window.read(|view, cx| {
        let dialog = view.add_model_dialog().unwrap();
        assert!(!dialog.pending());
        assert_eq!(
            dialog.context_window.read(cx).placeholder_text().as_ref(),
            "64000"
        );
    });
    assert!(!dir.exists());
}
