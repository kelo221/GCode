use super::*;
use crate::conversation::model::ConversationState;
use gpui::{AppContext, TestAppContext};
use serde_json::{Value, json};

fn live(s: &mut AppState, config: Value) -> std::sync::mpsc::Receiver<String> {
    let key = s.workspaces[0].key.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    s.workspaces[0].started = true;
    s.workspaces[0].pumping = true;
    s.workspaces[0].inbound = Some(tx);
    s.active_workspace = Some(key);
    s.active = Some("parent".into());
    s.draft = false;
    let mut conv = ConversationState::default();
    conv.apply_snapshot(&json!({"revision":1,"config":config,"inputRouting":{"mode":"enqueue"}}));
    s.conversations.insert("parent".into(), conv);
    rx
}

fn sent_mode(rx: &std::sync::mpsc::Receiver<String>) -> String {
    let request: Value = serde_json::from_str(&rx.try_recv().expect("a command")).unwrap();
    assert_eq!(request["params"]["type"], "switchCollaborationMode");
    request["params"]["payload"]["mode"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn config_mirrors_the_plan_overlay_apart_from_the_permission_mode() {
    let mut conv = ConversationState::default();
    conv.apply_config(&json!({"mode":"yolo","planEnabled":true,
        "planTransition":{"toolCallId":"call-1","planEnabled":true}}));
    assert_eq!(conv.config.mode, "yolo");
    assert!(conv.config.plan_enabled);
    assert_eq!(conv.config.plan_transition_id.as_deref(), Some("call-1"));
    conv.apply_config(&json!({"mode":"yolo","planEnabled":false}));
    assert!(!conv.config.plan_enabled);
    assert_eq!(conv.config.mode, "yolo", "leaving plan keeps Full access");
    // Legacy `mode: plan` turns the overlay on and keeps the permission.
    conv.apply_config(&json!({"mode":"plan"}));
    assert!(conv.config.plan_enabled);
    assert_eq!(conv.config.mode, "yolo");
}

#[gpui::test]
fn live_plan_toggle_keeps_the_permission_mode(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let rx = live(s, json!({"mode":"yolo","planEnabled":false}));
        let effective = s.effective_config();
        assert_eq!(
            (effective.mode.as_str(), effective.plan_enabled),
            ("yolo", false)
        );
        s.set_plan_enabled(true, cx);
        assert_eq!(sent_mode(&rx), "plan");
    });
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let rx = live(s, json!({"mode":"yolo","planEnabled":true}));
        let effective = s.effective_config();
        assert_eq!(
            (effective.mode.as_str(), effective.plan_enabled),
            ("yolo", true)
        );
        assert_eq!(mode_label(&effective.mode), "Full access");
        s.set_plan_enabled(false, cx);
        assert_eq!(sent_mode(&rx), "yolo");
    });
}

#[gpui::test]
fn permission_pick_while_planning_applies_now_and_keeps_plan(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let rx = live(s, json!({"mode":"build","planEnabled":true}));
        s.select_permission_mode("yolo", cx);
        let request: Value = serde_json::from_str(&rx.try_recv().expect("a command")).unwrap();
        assert_eq!(request["params"]["type"], "switchCollaborationMode");
        assert_eq!(
            request["params"]["payload"],
            json!({"mode":"yolo","planEnabled":true}),
            "{{mode}} alone would also clear plan"
        );
        assert_eq!(s.submission_override(), Value::Null, "no draft-only mode");
    });
}

#[gpui::test]
fn guarded_permission_round_trips(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let rx = live(s, json!({"mode":"build","planEnabled":false}));
        s.select_permission_mode("guarded", cx);
        assert_eq!(sent_mode(&rx), "guarded");
        s.conversations
            .get_mut("parent")
            .unwrap()
            .apply_config(&json!({"mode":"guarded"}));
        assert_eq!(s.effective_config().mode, "guarded");
        assert_eq!(mode_label("guarded"), "Autonomous mode");
    });
}

#[gpui::test]
fn new_task_plan_uses_the_workspace_default_permission(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        s.active_workspace = Some(key.clone());
        let mut catalog = crate::composer::catalog::WorkspaceConfig::default();
        catalog.apply_state(&json!({"configOptions":[{"id":"mode","currentValue":"yolo"}]}));
        s.workspace_configs.insert(key, catalog);
        s.set_plan_enabled(true, cx);
        assert!(s.ui_plan);
        let config = s.draft_config();
        assert_eq!(config["mode"], "yolo");
        assert_eq!(config["planEnabled"], true);
    });
}

#[gpui::test]
fn agent_plan_transition_drops_a_stale_override(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, _| {
        live(s, json!({"mode":"build","planEnabled":false}));
        s.draft_submission_overrides.insert(
            "parent".into(),
            json!({"mode":"build","planEnabled":true,"modelSelection":{"providerId":"p","modelId":"m"}}),
        );
        let before = s.plan_transition_before_frame("conversation/parent");
        s.reconcile_plan_override("parent", before.clone());
        assert_eq!(s.submission_override()["planEnabled"], true, "no transition yet");
        s.conversations.get_mut("parent").unwrap().apply_config(&json!({"mode":"build",
            "planEnabled":false,"planTransition":{"toolCallId":"exit-1","planEnabled":false}}));
        s.reconcile_plan_override("parent", before);
        assert_eq!(
            s.submission_override(),
            json!({"modelSelection":{"providerId":"p","modelId":"m"}})
        );
        assert!(!s.effective_config().plan_enabled);
        // A conversation that was not mirrored before keeps a restored draft.
        s.draft_submission_overrides
            .insert("parent".into(), json!({"planEnabled":true}));
        s.reconcile_plan_override("parent", None);
        assert_eq!(s.submission_override()["planEnabled"], true);
    });
}

#[test]
fn clicking_the_plan_chip_turns_plan_off_and_keeps_full_access() {
    use crate::app::{
        root::RootView,
        test_support::{RUNTIME_TEST_LOCK, TestTargets},
    };
    use gpui::{MouseButton, TestApp, px, size};
    use std::sync::Arc;
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = TestApp::with_text_system_and_assets(
        Arc::new(gpui::NoopTextSystem::new()),
        Arc::new(ely_gpui_component::Assets),
    );
    app.update(|cx| {
        ely_gpui_component::init(cx);
        cx.set_global(TestTargets::default());
        crate::shared::preferences::Preferences::install_at(
            Default::default(),
            std::env::temp_dir().join(uuid::Uuid::now_v7().to_string()),
            Arc::new(|| true),
            cx,
        );
    });
    let state = app.new_entity(AppState::for_test);
    let rx = app.update_entity(&state, |s, _| {
        live(s, json!({"mode":"yolo","planEnabled":true}))
    });
    let mut window = app.open_window(|_, cx| RootView::new(state.clone(), cx));
    window.simulate_resize(size(px(1280.), px(820.)));
    window.update(|_, window, cx| window.bounds_changed(cx));
    window.draw();
    let chip = app.update(|cx| cx.global::<TestTargets>().0.get("plan-marker").copied());
    let chip = chip.expect("the Plan chip shows while plan is on");
    while rx.try_recv().is_ok() {}
    window.simulate_click(chip.center(), MouseButton::Left);
    window.draw();
    let modes: Vec<String> = rx
        .try_iter()
        .filter_map(|raw| serde_json::from_str::<Value>(&raw).ok())
        .filter(|r| r["params"]["type"] == "switchCollaborationMode")
        .map(|r| {
            r["params"]["payload"]["mode"]
                .as_str()
                .unwrap_or("")
                .to_string()
        })
        .collect();
    assert_eq!(modes, ["yolo"]);
}
