use crate::app::{
    root::RootView,
    store::AppState,
    test_support::{RUNTIME_TEST_LOCK, TestTargets},
};
use crate::conversation::model::ConversationState;
use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, TestApp, px, size};
use serde_json::{Value, json};
use std::sync::Arc;

#[test]
fn delivery_keyboard_and_modifier_pointer_reach_same_sender() {
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
    app.update_entity(&state, |s, _| {
        let key = s.workspaces[0].key.clone();
        s.workspaces[0].inbound = Some(tx);
        s.workspaces[0].started = true;
        s.workspaces[0].pumping = true;
        s.active_workspace = Some(key);
        s.active = Some("session".into());
        s.draft = false;
        let mut conv = ConversationState {
            subscribed: true,
            ..Default::default()
        };
        conv.apply_snapshot(&json!({"revision":1,"control":{"phase":"running","canStop":true},"inputRouting":{"mode":"enqueue"},"config":{"followupMode":"queue"}}));
        s.conversations.insert("session".into(), conv);
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
    while rx.try_recv().is_ok() {}
    window.simulate_input("keyboard");
    window.simulate_keystroke(if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    });
    let command: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    assert_eq!(
        command["params"]["payload"]["requestedDelivery"],
        "startNow"
    );
    app.update_entity(&state, |s, cx| {
        s.conversations
            .get_mut("session")
            .unwrap()
            .config
            .followup_mode = "guide".into();
        s.composer.update(cx, |c, _| c.set_text("pointer"));
    });
    window.draw();
    let target = app.read(|cx| cx.global::<TestTargets>().0["send-btn"].center());
    let modifiers = if cfg!(target_os = "macos") {
        Modifiers {
            platform: true,
            ..Default::default()
        }
    } else {
        Modifiers {
            control: true,
            ..Default::default()
        }
    };
    window.simulate_event(MouseDownEvent {
        position: target,
        button: MouseButton::Left,
        modifiers,
        click_count: 1,
        first_mouse: false,
    });
    window.simulate_event(MouseUpEvent {
        position: target,
        button: MouseButton::Left,
        modifiers,
        click_count: 1,
    });
    let command: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    assert_eq!(command["params"]["payload"]["requestedDelivery"], "queue");
    app.read_entity(&state, |s, _| {
        assert_eq!(s.conversations["session"].config.followup_mode, "guide")
    });
    app.update_entity(&state, |s, cx| {
        s.conversations.get_mut("session").unwrap().input_routing =
            Some(crate::composer::delivery::InputRouting::Reject);
        s.composer.update(cx, |c, _| c.set_text("keep rejected"));
    });
    window.update(|_, window, cx| {
        let focus = state.read(cx).composer.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    window.simulate_keystroke(if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    });
    assert!(rx.try_recv().is_err());
    app.read_entity(&state, |s, cx| {
        assert_eq!(s.composer.read(cx).text(), "keep rejected")
    });
    window.simulate_keystroke("shift-enter");
    app.read_entity(&state, |s, cx| {
        assert_eq!(s.composer.read(cx).text(), "keep rejected\n")
    });
    app.update_entity(&state, |s, cx| {
        let conv = s.conversations.get_mut("session").unwrap();
        conv.input_routing = Some(crate::composer::delivery::InputRouting::Enqueue);
        conv.config.followup_mode = "queue".into();
        s.composer.update(cx, |c, _| {
            c.set_text("composing");
            c.marked_utf16 = Some(0..9);
        });
    });
    window.simulate_keystroke(if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    });
    assert!(rx.try_recv().is_err());
    app.read_entity(&state, |s, cx| {
        assert_eq!(s.composer.read(cx).text(), "composing");
        assert_eq!(s.composer.read(cx).marked_utf16, Some(0..9));
    });
    app.update_entity(&state, |s, cx| {
        s.composer
            .update(cx, |c, _| c.set_text("ordinary modifier"))
    });
    window.simulate_keystroke(if cfg!(target_os = "macos") {
        "ctrl-enter"
    } else {
        "cmd-enter"
    });
    let command: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    assert_eq!(command["params"]["type"], "sendText");
    assert!(
        command["params"]["payload"]
            .get("requestedDelivery")
            .is_none()
    );
    assert!(rx.try_recv().is_err());
}

#[test]
fn running_session_shows_working_stop_and_hides_send_until_there_is_a_draft() {
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
    app.update_entity(&state, |s, _| {
        let key = s.workspaces[0].key.clone();
        s.active_workspace = Some(key);
        s.active = Some("session".into());
        s.draft = false;
        let mut conv = ConversationState {
            subscribed: true,
            ..Default::default()
        };
        conv.apply_snapshot(&json!({"revision":1,"control":{"phase":"running","canStop":true},"inputRouting":{"mode":"enqueue"}}));
        s.conversations.insert("session".into(), conv);
    });
    let mut window = app.open_window(|_, cx| RootView::new(state.clone(), cx));
    window.simulate_resize(size(px(1280.), px(820.)));
    window.update(|_, window, cx| window.bounds_changed(cx));
    macro_rules! buttons {
        () => {{
            app.update(|cx| cx.global_mut::<TestTargets>().0.clear());
            window.draw();
            app.read(|cx| {
                let t = &cx.global::<TestTargets>().0;
                (t.contains_key("stop-btn"), t.contains_key("send-btn"))
            })
        }};
    }
    assert_eq!(buttons!(), (true, false), "running, empty draft");
    app.update_entity(&state, |s, cx| {
        s.composer.update(cx, |c, _| c.set_text("next"))
    });
    assert_eq!(buttons!(), (true, true), "running, with a draft");
    app.update_entity(&state, |s, _| {
        s.conversations
            .get_mut("session")
            .unwrap()
            .apply_control(&json!({"phase":"completedSuccess","canStop":false}));
    });
    assert_eq!(buttons!(), (false, true), "idle");
}

#[test]
fn context_ring_leads_the_composer_controls_only_with_usage() {
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
    let usage = |cw: Value| {
        json!({ "revision": 1, "usage": { "contextWindow": cw, "cumulative": {
            "inputTokens": 0, "outputTokens": 0, "cacheReadTokens": 0, "cacheWriteTokens": 0 } } })
    };
    app.update_entity(&state, |s, _| {
        let key = s.workspaces[0].key.clone();
        s.active_workspace = Some(key);
        s.active = Some("session".into());
        s.draft = false;
        let mut conv = ConversationState {
            subscribed: true,
            ..Default::default()
        };
        conv.apply_snapshot(&usage(Value::Null));
        s.conversations.insert("session".into(), conv);
    });
    let mut window = app.open_window(|_, cx| RootView::new(state.clone(), cx));
    window.simulate_resize(size(px(1280.), px(820.)));
    window.update(|_, window, cx| window.bounds_changed(cx));
    macro_rules! ring {
        () => {{
            app.update(|cx| cx.global_mut::<TestTargets>().0.clear());
            window.draw();
            app.read(|cx| cx.global::<TestTargets>().0.get("context-ring").copied())
        }};
    }
    assert_eq!(ring!(), None, "no context window yet");
    app.update_entity(&state, |s, _| {
        s.conversations
            .get_mut("session")
            .unwrap()
            .apply_snapshot(&usage(json!({
                "usedTokens": 40_000, "maxTokens": 200_000, "autoCompactThresholdTokens": null
            })));
    });
    let ring = ring!().expect("ring with usage");
    assert_eq!(ring.size, size(px(28.), px(28.)));
    let others = app.read(|cx| {
        let t = &cx.global::<TestTargets>().0;
        ["model-menu-row", "thought-menu-row"]
            .iter()
            .filter_map(|id| t.get(*id).copied())
            .collect::<Vec<_>>()
    });
    assert!(
        others.iter().all(|b| b.origin.x > ring.origin.x),
        "ring comes first"
    );
}
