use super::{highlighted, step};
use crate::app::{
    root::RootView,
    store::AppState,
    test_support::{RUNTIME_TEST_LOCK, TestTargets},
};
use crate::composer::slash_catalog::tests::commands;
use crate::conversation::model::ConversationState;
use gpui::{Entity, TestApp, TestAppWindow, px, size};
use serde_json::{Value, json};
use std::sync::Arc;

#[test]
fn highlight_follows_the_text_it_was_picked_for_and_wraps() {
    let pick = ("/".to_string(), 1);
    assert_eq!(highlighted(&pick, "/", 2), 1);
    assert_eq!(highlighted(&pick, "/i", 2), 0, "new text starts at the top");
    assert_eq!(highlighted(&pick, "/", 1), 0, "clamped to the rows shown");
    assert_eq!(highlighted(&pick, "/", 0), 0);
    assert_eq!(step(1, 1, 2), 0);
    assert_eq!(step(0, -1, 2), 1);
}

/// A focused composer with the two-command catalog loaded, its text set to `text`.
fn loaded(text: &str) -> (TestApp, Entity<AppState>, TestAppWindow<RootView>) {
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
    let (tx, rx) = std::sync::mpsc::channel();
    let key = app.update_entity(&state, |s, cx| {
        let key = s.workspaces[0].key.clone();
        s.workspaces[0].inbound = Some(tx);
        s.workspaces[0].started = true;
        s.workspaces[0].pumping = true;
        s.active_workspace = Some(key.clone());
        s.active = Some("parent".into());
        s.draft = false;
        let mut conversation = ConversationState {
            subscribed: true,
            ..Default::default()
        };
        conversation.apply_snapshot(
            &json!({"revision":1,"inputRouting":{"mode":"startNow"},"control":{"canStop":false}}),
        );
        s.conversations.insert("parent".into(), conversation);
        s.composer.update(cx, |c, _| c.set_text(text));
        key
    });
    let mut window = app.open_window(|window, cx| {
        let view = RootView::new(state.clone(), cx);
        let focus = state.read(cx).composer.read(cx).focus.clone();
        window.focus(&focus, cx);
        view
    });
    window.simulate_resize(size(px(1280.), px(820.)));
    window.draw();
    // Only slash text asks for the catalog.
    if let Ok(request) = rx.try_recv() {
        let request: Value = serde_json::from_str(&request).unwrap();
        app.update_entity(&state, |s, cx| {
            let result = json!({"session":{"sessionId":"parent","workspace":{"workspacePath":s.workspaces[0].path.to_string_lossy(),"workspaceKey":key}},"slashCommands":commands()});
            s.handle_response(&key, request["id"].as_u64().unwrap(), Some(result), None, cx);
        });
    }
    window.draw();
    (app, state, window)
}

fn text(app: &TestApp, state: &Entity<AppState>) -> String {
    app.read_entity(state, |s, cx| s.composer.read(cx).text().to_string())
}

#[test]
fn tab_completes_the_only_matching_command() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (app, state, mut window) = loaded("/in");
    window.simulate_keystroke("tab");
    assert_eq!(text(&app, &state), "/init ");
}

#[test]
fn down_then_tab_completes_the_highlighted_command() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, state, mut window) = loaded("/");
    window.simulate_keystroke("down");
    window.draw();
    window.simulate_keystroke("tab");
    assert_eq!(text(&app, &state), "/review-local ");
    // A fresh popup starts at the top again; Up wraps to the last row.
    app.update_entity(&state, |s, cx| {
        s.composer.update(cx, |c, _| c.set_text("/"))
    });
    window.draw();
    window.simulate_keystroke("up");
    window.simulate_keystroke("tab");
    assert_eq!(text(&app, &state), "/review-local ");
}

#[test]
fn tab_without_suggestions_is_not_taken_by_the_popup() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (app, state, mut window) = loaded("plain words");
    window.simulate_keystroke("tab");
    // Tab reaches the composer's ordinary input handling unchanged.
    assert_eq!(text(&app, &state), "plain words\t");
}
