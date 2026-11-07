use crate::app::store::AppState;
use crate::conversation::model::ConversationState;
use gpui::{AppContext, Context, TestAppContext};
use serde_json::Value;
use std::sync::mpsc::Receiver;

/// Workspace 0 is spawning (pumping, not started); `session` is open.
fn sleeping(s: &mut AppState, session: Option<&str>) -> String {
    let key = s.workspaces[0].key.clone();
    s.workspaces[0].pumping = true;
    s.active_workspace = Some(key.clone());
    if let Some(sid) = session {
        s.active = Some(sid.into());
        s.draft = false;
        s.conversations
            .insert(sid.into(), ConversationState::default());
    }
    key
}

fn start(s: &mut AppState, cx: &mut Context<AppState>) -> Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    s.workspaces[0].inbound = Some(tx);
    s.workspaces[0].started = true;
    s.advance_waking_send(cx);
    rx
}

fn types(rx: &Receiver<String>) -> Vec<String> {
    rx.try_iter()
        .map(|l| serde_json::from_str::<Value>(&l).unwrap())
        .map(|v| match v["params"]["type"].as_str() {
            Some(t) => t.to_owned(),
            None => v["method"].as_str().unwrap_or_default().to_owned(),
        })
        .collect()
}

fn send(s: &mut AppState, text: &str, cx: &mut Context<AppState>) {
    s.composer.update(cx, |c, _| c.set_text(text));
    s.submit_composer(cx);
}

#[gpui::test]
fn parked_send_waits_for_start_and_subscribe_then_sends_once(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        sleeping(s, Some("s1"));
        send(s, "hello", cx);
        send(s, "and more", cx);
        assert!(s.waking_send_visible());
        assert_eq!(s.composer.read(cx).text(), "");
        let rx = start(s, cx);
        assert_eq!(types(&rx), ["v4/conversation/subscribe"]);
        s.advance_waking_send(cx);
        assert!(types(&rx).is_empty(), "no resubscribe while one is pending");
        s.workspaces[0].pending.clear();
        s.conversations.get_mut("s1").unwrap().subscribed = true;
        s.advance_waking_send(cx);
        let sent: Vec<Value> = rx
            .try_iter()
            .map(|l| serde_json::from_str(&l).unwrap())
            .collect();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["params"]["type"], "sendText");
        assert_eq!(sent[0]["params"]["payload"]["text"], "hello\nand more");
        assert!(s.waking_send.is_none());
    });
}

#[gpui::test]
fn new_chat_sends_as_soon_as_the_agent_started(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        sleeping(s, None);
        send(s, "new task", cx);
        let rx = start(s, cx);
        assert_eq!(types(&rx), ["createSession"]);
        assert!(s.waking_send.is_none());
    });
}

#[gpui::test]
fn failed_subscribe_and_cancel_restore_the_draft(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        sleeping(s, Some("s1"));
        send(s, "first", cx);
        let rx = start(s, cx);
        // The subscribe answer arrived without a subscription.
        s.workspaces[0].pending.clear();
        s.advance_waking_send(cx);
        assert_eq!(types(&rx), ["v4/conversation/subscribe"]);
        assert!(s.waking_send.is_none());
        assert_eq!(s.composer.read(cx).text(), "first");

        s.composer.update(cx, |c, _| c.set_text(""));
        s.workspaces[0].started = false;
        s.workspaces[0].inbound = None;
        send(s, "second", cx);
        s.give_up_waking_send("cancelled", cx);
        assert_eq!(s.composer.read(cx).text(), "second");
    });
}

#[gpui::test]
fn agent_that_never_starts_restores_the_draft(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        sleeping(s, Some("s1"));
        send(s, "lost?", cx);
        s.workspaces[0].status = "backend down".into();
        s.advance_waking_send(cx);
        assert!(s.waking_send.is_none());
        assert_eq!(s.composer.read(cx).text(), "lost?");
        assert!(s.errors.iter().any(|e| e.contains("did not start")));
    });
}
