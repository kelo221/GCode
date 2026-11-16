use crate::app::{
    root::RootView,
    store::AppState,
    test_support::{RUNTIME_TEST_LOCK, TestTargets},
};
use crate::conversation::interactions::PendingInteraction;
use crate::conversation::model::ConversationState;
use gpui::{Entity, TestApp, TestAppWindow, px, size};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

fn open(
    pending: Value,
) -> (
    TestApp,
    TestAppWindow<RootView>,
    Entity<AppState>,
    Receiver<String>,
) {
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
        let key = s.workspaces[0].key.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        s.workspaces[0].started = true;
        s.workspaces[0].pumping = true;
        s.workspaces[0].inbound = Some(tx);
        s.active_workspace = Some(key);
        s.active = Some("s1".into());
        s.draft = false;
        let mut conv = ConversationState::default();
        conv.apply_snapshot(&json!({"revision":1,"config":{"mode":"build"},
            "control":{"phase":"running","canStop":true},
            "rows":{"firstRowId":1,"totalCount":1,"window":[
                {"rowId":1,"kind":"userInput","text":"hi","origin":"realUser"}]}}));
        conv.pending_interactions = vec![PendingInteraction::from_value(&pending).unwrap()];
        s.conversations.insert("s1".into(), conv);
        rx
    });
    let mut window = app.open_window(|_, cx| RootView::new(state.clone(), cx));
    window.simulate_resize(size(px(1280.), px(820.)));
    window.update(|_, window, cx| window.bounds_changed(cx));
    window.draw();
    window.draw();
    (app, window, state, rx)
}

fn answers(rx: &Receiver<String>) -> Vec<Value> {
    rx.try_iter()
        .filter_map(|raw| serde_json::from_str::<Value>(&raw).ok())
        .filter(|r| r["params"]["type"] == "resolveInteraction")
        .map(|r| r["params"]["payload"]["answer"].clone())
        .collect()
}

fn input_focused(window: &mut TestAppWindow<RootView>) -> (bool, bool) {
    window.update(|root, window, cx| {
        let card = root
            .interaction_input
            .input
            .read(cx)
            .focus
            .is_focused(window);
        let composer = root.state.read(cx).composer.read(cx).focus.clone();
        (card, composer.is_focused(window))
    })
}

#[test]
fn typed_answers_step_through_questions_and_submit_in_the_card() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_app, mut window, state, rx) = open(json!({
        "interactionId": "ask-1", "kind": "userInput",
        "payload": { "kind": "userInput", "toolName": "AskUserQuestion", "prompt": "",
            "questions": [
                { "question": "Which DB?", "header": "DB", "multiSelect": false,
                  "options": [ { "label": "Postgres" }, { "label": "MySQL" } ] },
                { "question": "Extras?", "header": "Ext", "multiSelect": true,
                  "options": [ { "label": "Cache" } ] } ] } }));
    // 问题卡片替代了输入框：主输入框不再绘制，焦点在卡片输入框里。
    window.read(|root, cx| {
        let composer = root.state.read(cx).composer.clone();
        assert!(composer.read(cx).view.caret_bounds.get().is_none());
    });
    assert_eq!(input_focused(&mut window), (true, false));
    window.simulate_input("SQLite");
    window.simulate_keystroke("enter");
    window.draw();
    window.read(|root, _| assert_eq!(root.question_steps["ask-1"], 1));
    window.simulate_input("Queue");
    window.simulate_keystroke("enter");
    window.draw();
    assert_eq!(
        answers(&rx),
        [json!({ "action": "accept", "content": {
            "answers": { "Which DB?": "SQLite", "Extras?": "Queue" },
            "answer_0": "SQLite", "answer_1": ["Queue"] } })]
    );
    window.draw();
    window.read(|root, cx| {
        assert!(
            root.state
                .read(cx)
                .active_conversation()
                .unwrap()
                .pending_interactions
                .is_empty()
        );
        assert!(
            root.state
                .read(cx)
                .composer
                .read(cx)
                .view
                .caret_bounds
                .get()
                .is_some()
        );
    });
    assert_eq!(input_focused(&mut window), (false, true));
    drop(state);
}

#[test]
fn going_back_keeps_each_question_draft_and_a_pick_clears_typed_text() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_app, mut window, _state, _rx) = open(json!({
        "interactionId": "ask-2", "kind": "userInput",
        "payload": { "kind": "userInput", "prompt": "",
            "questions": [
                { "question": "A?", "options": [ { "label": "x" } ] },
                { "question": "B?", "options": [ { "label": "y" } ] } ] } }));
    window.simulate_input("first");
    window.simulate_keystroke("enter");
    window.draw();
    window.simulate_input("second");
    window.update(|root, _, cx| {
        root.question_steps.insert("ask-2".into(), 0);
        cx.notify();
    });
    window.draw();
    window.read(|root, cx| {
        assert_eq!(root.interaction_input.input.read(cx).text(), "first");
        assert_eq!(root.typed_text("ask-2", 1, cx), "second");
    });
    window.update(|root, _, cx| root.clear_typed_text("ask-2", 0, cx));
    window.read(|root, cx| assert_eq!(root.typed_text("ask-2", 0, cx), ""));
}

#[test]
fn permission_feedback_denies_with_the_typed_text() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_app, mut window, _state, rx) = open(json!({
        "interactionId": "perm-1", "kind": "permission",
        "payload": { "kind": "permission", "toolName": "Bash", "summary": "Run rm",
            "freeText": true,
            "options": [ { "optionId": "allow", "label": "Allow", "kind": "allowOnce" },
                         { "optionId": "deny", "label": "Deny", "kind": "deny" } ] } }));
    assert_eq!(input_focused(&mut window), (true, false));
    window.simulate_keystroke("enter");
    window.draw();
    assert!(answers(&rx).is_empty(), "an empty Enter never denies");
    window.simulate_input("use trash instead");
    window.simulate_keystroke("enter");
    window.draw();
    assert_eq!(
        answers(&rx),
        [json!({ "optionId": "deny", "freeText": "use trash instead" })]
    );
}

/// Unknown kinds never take over the composer: their card has no answer
/// input, so hiding the composer behind it would leave the user stuck.
#[test]
fn only_permission_and_questions_block_the_composer() {
    let pi = |kind: &str| {
        PendingInteraction::from_value(&json!({
            "interactionId": kind, "kind": kind,
            "payload": { "kind": kind, "summary": "", "prompt": "" }
        }))
        .unwrap()
    };
    let pending = [
        pi("pluginInstall"),
        pi("workspaceHookReview"),
        pi("userInput"),
    ];
    assert_eq!(
        crate::conversation::interaction_input::blocking(&pending).map(|p| p.kind.as_str()),
        Some("userInput")
    );
    assert!(crate::conversation::interaction_input::blocking(&pending[..2]).is_none());
}
