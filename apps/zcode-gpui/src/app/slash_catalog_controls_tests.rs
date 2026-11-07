use crate::app::{
    root::RootView,
    store::AppState,
    test_support::{RUNTIME_TEST_LOCK, TestTargets},
};
use crate::composer::{attachment::AttachmentRef, slash_catalog::tests::commands};
use crate::conversation::model::ConversationState;
use gpui::{MouseButton, TestApp, px, size};
use serde_json::{Value, json};
use std::sync::Arc;

#[test]
fn slash_catalog_init_selection_refresh_and_submission_are_pointer_keyboard_reachable() {
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
        s.composer.update(cx, |c, _| {
            c.set_text("/in");
            c.attachments = vec![AttachmentRef {
                reference: "retained".into(),
                file_name: "notes.txt".into(),
                mime: "text/plain".into(),
                bytes: 1,
                preview_ref: None,
            }];
        });
        key
    });
    let mut window = app.open_window(|window, cx| {
        let view = RootView::new(state.clone(), cx);
        let focus = state.read(cx).composer.read(cx).focus.clone();
        window.focus(&focus, cx);
        view
    });
    window.simulate_resize(size(px(1280.), px(820.)));
    window.update(|_, window, cx| window.bounds_changed(cx));
    window.draw();
    let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    assert_eq!(request["method"], "session/read");
    assert_eq!(
        request["params"],
        json!({"sessionId":"parent","messageLimit":1})
    );
    app.update_entity(&state, |s, cx| {
        let result = json!({"session":{"sessionId":"parent","workspace":{"workspacePath":s.workspaces[0].path.to_string_lossy(),"workspaceKey":key}},"slashCommands":commands()});
        s.handle_response(&key, request["id"].as_u64().unwrap(), Some(result), None, cx);
    });
    window.draw();
    let target = app.read(|cx| cx.global::<TestTargets>().0["auto-item-0"].center());
    window.simulate_click(target, MouseButton::Left);
    app.read_entity(&state, |s, cx| {
        assert_eq!(s.composer.read(cx).text(), "/init ");
        assert_eq!(s.composer.read(cx).attachments()[0].reference, "retained");
    });
    assert!(rx.try_recv().is_err());
    window.update(|_, window, cx| {
        let focus = state.read(cx).composer.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    window.simulate_input("extra notes");
    window.simulate_keystroke("enter");
    let sent: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    assert_eq!(sent["params"]["type"], "sendText");
    assert_eq!(sent["params"]["payload"]["text"], "/init extra notes");
    assert_eq!(
        sent["params"]["payload"]["attachments"][0]["ref"],
        "retained"
    );
    app.update_entity(&state, |s, cx| {
        s.composer.update(cx, |c, _| c.set_text("/"));
    });
    window.draw();
    let refresh = app.read(|cx| cx.global::<TestTargets>().0["refresh-slash-catalog"].center());
    window.simulate_click(refresh, MouseButton::Left);
    let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    assert_eq!(request["method"], "session/read");
    app.update_entity(&state, |s, cx| {
        let result = json!({"session":{"sessionId":"parent","workspace":{"workspacePath":s.workspaces[0].path.to_string_lossy(),"workspaceKey":key}},"slashCommands":[]});
        s.handle_response(&key, request["id"].as_u64().unwrap(), Some(result), None, cx);
        assert!(s.active_slash_commands().unwrap().is_empty());
        assert_eq!(s.composer.read(cx).text(), "/");
    });
    window.draw();
    window.update(|view, _, cx| assert!(view.autocomplete_suggestions(cx).is_none()));
    assert!(rx.try_recv().is_err());
    app.update_entity(&state, |s, cx| {
        s.composer
            .update(cx, |c, _| c.set_text("/plan pointer task"))
    });
    window.draw();
    let send = app.read(|cx| cx.global::<TestTargets>().0["send-btn"].center());
    window.simulate_click(send, MouseButton::Left);
    let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    assert_eq!(request["params"]["type"], "sendText");
    assert_eq!(request["params"]["payload"]["text"], "pointer task");
    assert_eq!(request["params"]["payload"]["planEnabled"], true);
    window.update(|_, window, cx| {
        let focus = state.read(cx).composer.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    window.simulate_input("/plan");
    window.simulate_keystroke("enter");
    assert!(rx.try_recv().is_err());
    app.read_entity(&state, |s, cx| {
        assert!(s.composer.read(cx).text().is_empty());
        assert!(s.effective_config().plan_enabled);
    });
}
