use super::*;
use std::collections::HashMap;

#[test]
fn read_only_view_predicate() {
    assert!(!is_read_only(None));
    assert!(is_read_only(Some("child-1")));
}

#[test]
fn parent_session_resolution() {
    let mut child_owner = HashMap::new();
    child_owner.insert(
        "child-1".to_string(),
        ("ws-1".to_string(), "parent-1".to_string()),
    );

    assert_eq!(parent_session_id(&child_owner, None), None);
    assert_eq!(
        parent_session_id(&child_owner, Some("child-1")),
        Some("parent-1")
    );
    assert_eq!(parent_session_id(&child_owner, Some("child-unknown")), None);
}

#[test]
fn child_topic_workspace_routing() {
    let mut child_owner = HashMap::new();
    child_owner.insert(
        "child-42".to_string(),
        ("ws-xyz".to_string(), "parent-1".to_string()),
    );

    assert_eq!(
        child_topic_workspace(&child_owner, "conversation/child-42"),
        Some("ws-xyz".to_string())
    );
    assert_eq!(
        child_topic_workspace(&child_owner, "conversation/unknown-child"),
        None
    );
    assert_eq!(child_topic_workspace(&child_owner, "status/child-42"), None);
}

#[test]
fn active_conversation_sid_fallback() {
    assert_eq!(active_conversation_sid(None, None), None);
    assert_eq!(
        active_conversation_sid(None, Some("parent-1")),
        Some("parent-1")
    );
    assert_eq!(
        active_conversation_sid(Some("child-1"), Some("parent-1")),
        Some("child-1")
    );
    assert_eq!(
        active_conversation_sid(Some("child-1"), None),
        Some("child-1")
    );
}

#[gpui::test]
fn load_earlier_pages_the_viewed_subagent_not_the_parent(cx: &mut gpui::TestAppContext) {
    use crate::app::store::AppState;
    use crate::conversation::model::ConversationState;
    use gpui::AppContext;
    use serde_json::{Value, json};
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        s.workspaces[0].started = true;
        s.workspaces[0].pumping = true;
        s.workspaces[0].inbound = Some(tx);
        s.active_workspace = Some(key.clone());
        s.active = Some("parent".into());
        s.draft = false;
        // Parent: its whole history is loaded. Child: older rows remain.
        let mut parent = ConversationState::default();
        parent.apply_snapshot(&json!({"revision":1,"rows":{"firstRowId":1,"totalCount":1,
            "window":[{"rowId":1,"kind":"userInput","text":"p"}]}}));
        parent.subscribed = true;
        let mut child = ConversationState::default();
        child.apply_snapshot(&json!({"revision":1,"rows":{"firstRowId":1,"totalCount":50,
            "window":[{"rowId":40,"kind":"userInput","text":"c"}]}}));
        child.subscribed = true;
        s.conversations.insert("parent".into(), parent);
        s.conversations.insert("child".into(), child);
        s.child_owner
            .insert("child".into(), (key.clone(), "parent".into()));
        s.viewing_child = Some("child".into());
        s.fetch_earlier_rows(cx);
        let request: Value = serde_json::from_str(&rx.try_recv().expect("a page request")).unwrap();
        assert_eq!(request["method"], "v4/conversation/rowsRange");
        assert_eq!(request["params"]["sessionId"], "child");
        assert_eq!(request["params"]["beforeRowId"], 40);
    });
}

#[test]
fn child_session_id_collection() {
    let mut child_owner = HashMap::new();
    child_owner.insert(
        "child-owner-1".to_string(),
        ("ws".to_string(), "p".to_string()),
    );

    let sub_state = crate::conversation::subagents::SubagentsState {
        child_session_ids: vec!["subagent-child-2".to_string(), "shared-child".to_string()],
        ..Default::default()
    };
    child_owner.insert(
        "shared-child".to_string(),
        ("ws".to_string(), "p".to_string()),
    );

    let collected = collect_child_session_ids(&child_owner, [&sub_state]);
    assert_eq!(collected.len(), 3);
    assert!(collected.contains("child-owner-1"));
    assert!(collected.contains("subagent-child-2"));
    assert!(collected.contains("shared-child"));
}
