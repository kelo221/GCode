use crate::app::root::RootView;
use crate::app::settings::SettingsSection;
use crate::app::store::AppState;
use crate::app::test_support::{RUNTIME_TEST_LOCK, TestTargets};
use crate::backend::launcher::ConnEvent;
use crate::shared::preferences::{PreferenceChange, Preferences};
use crate::shared::settings::{AppSettings, load_settings_from_path};
use gpui::{MouseButton, TestApp, px, size};
use serde_json::{Value, json};
use std::sync::{Arc, mpsc};

fn request(id: u64) -> ConnEvent {
    ConnEvent::Line(
        json!({"id":id,"method":"session/requestRuntimePreferences",
        "params":{"sessionId":"fixture-session","scope":"runtime-materialization"}})
        .to_string(),
    )
}

#[test]
fn tool_search_mode_is_picked_in_the_mcp_pane_and_served_to_new_sessions() {
    assert_eq!(AppSettings::default().tool_search_mode(), "off");
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("gpui-tool-search-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("setting.json");
    std::fs::write(&path, r#"{"toolSearchMode":"off","unmodeled":true}"#).unwrap();
    let mut app = TestApp::with_text_system_and_assets(
        Arc::new(gpui::NoopTextSystem::new()),
        Arc::new(ely_gpui_component::Assets),
    );
    app.update(|cx| {
        ely_gpui_component::init(cx);
        cx.set_global(TestTargets::default());
        Preferences::install_at(
            load_settings_from_path(&path),
            path.clone(),
            Arc::new(|| false),
            cx,
        );
    });
    let state = app.new_entity(AppState::for_test);
    let (tx, rx) = mpsc::channel();
    let key = app.update_entity(&state, |s, _| {
        s.workspaces[0].inbound = Some(tx);
        s.workspaces[0].key.clone()
    });
    let served = |app: &mut TestApp, id: u64| -> Value {
        app.update_entity(&state, |s, cx| {
            assert!(s.handle_conn(&key, request(id), cx))
        });
        let reply: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        reply["result"]["toolSearchMode"].clone()
    };
    assert_eq!(served(&mut app, 1), "off");

    let mut window = app.open_window(|window, cx| {
        let mut view = RootView::new(state.clone(), cx);
        view.open_settings(window, cx);
        view
    });
    window.simulate_resize(size(px(1280.), px(820.)));
    window.draw();
    let section = app.read(|cx| cx.global::<TestTargets>().0["settings-mcp"].center());
    window.simulate_click(section, MouseButton::Left);
    window.read(|v, _| assert!(v.settings.section == SettingsSection::Mcp));
    for (id, (key, expected)) in [("home", "auto"), ("down", "local"), ("end", "off")]
        .into_iter()
        .enumerate()
    {
        window.draw();
        let select =
            app.read(|cx| cx.global::<TestTargets>().0["settings-tool-search-mode"].center());
        window.simulate_click(select, MouseButton::Left);
        window.simulate_keystroke(key);
        window.simulate_keystroke("enter");
        let saved = load_settings_from_path(&path);
        assert_eq!(saved.tool_search_mode.as_deref(), Some(expected), "{key}");
        assert_eq!(saved.extra["unmodeled"], true);
        assert_eq!(served(&mut app, id as u64 + 2), expected);
    }
    drop(window);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unknown_tool_search_modes_are_rejected_before_saving() {
    let change = PreferenceChange::ToolSearchMode("native".into());
    assert!(change.validate(&AppSettings::default()).is_err());
    let ok = PreferenceChange::ToolSearchMode("local".into());
    assert!(ok.validate(&AppSettings::default()).is_ok());
}

#[test]
fn unknown_codemode_modes_are_rejected_before_saving() {
    let change = PreferenceChange::CodemodeMode("auto".into());
    assert!(change.validate(&AppSettings::default()).is_err());
    let ok = PreferenceChange::CodemodeMode("only".into());
    assert!(ok.validate(&AppSettings::default()).is_ok());
}
