use crate::app::store::AppState;
use crate::composer::slash_catalog::{SLASH_CATALOG_ERROR, parse_commands};
use gpui::{AppContext, TestAppContext};
use serde_json::{Value, json};

fn response(s: &AppState, session: Option<&str>, commands: Value) -> Value {
    let workspace = json!({"workspacePath":s.workspaces[0].path.to_string_lossy(),"workspaceKey":s.workspaces[0].key});
    match session {
        Some(session) => {
            json!({"session":{"sessionId":session,"workspace":workspace},"slashCommands":commands,"messages":[{"text":"never import legacy rows"}]})
        }
        None => json!({"workspace":workspace,"mode":"build","slashCommands":commands}),
    }
}

#[gpui::test]
fn slash_catalog_queries_use_exact_scope_and_never_session_workspace_fallback(
    cx: &mut TestAppContext,
) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        s.workspaces[0].started = true;
        s.workspaces[0].inbound = Some(tx);
        s.active_workspace = Some(key.clone());
        s.composer.update(cx, |c, _| c.set_text("/in"));
        s.ensure_slash_catalog(false, cx);
        s.ensure_slash_catalog(false, cx);
        let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(request["method"], "workspace/readPresentation");
        assert_eq!(request["params"], json!({"workspace":{"workspacePath":s.workspaces[0].path.to_string_lossy(),"workspaceKey":key}}));
        assert!(rx.try_recv().is_err());
        s.handle_response(&key, request["id"].as_u64().unwrap(), Some(response(s, None, crate::composer::slash_catalog::tests::commands())), None, cx);
        assert_eq!(s.active_slash_commands().unwrap()[0].name, "init");
        s.active = Some("parent".into());
        s.draft = false;
        assert!(s.active_slash_commands().is_none());
        // Not yet loaded on the agent (e.g. right after a wake): no doomed session/read.
        s.ensure_slash_catalog(false, cx);
        assert!(rx.try_recv().is_err());
        assert!(s.active_slash_query().is_none());
        s.conversations.entry("parent".into()).or_default().subscribed = true;
        s.ensure_slash_catalog(false, cx);
        let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(request["method"], "session/read");
        assert_eq!(request["params"], json!({"sessionId":"parent","messageLimit":1}));
        s.handle_response(&key, request["id"].as_u64().unwrap(), Some(response(s, Some("parent"), json!([]))), None, cx);
        assert!(s.active_slash_commands().unwrap().is_empty());
        assert!(s.conversations["parent"].rows.is_empty());
        assert_eq!(s.composer.read(cx).text(), "/in");
        s.ensure_slash_catalog(false, cx);
        assert!(rx.try_recv().is_err());
        s.ensure_slash_catalog(true, cx);
        let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        s.handle_response(&key, request["id"].as_u64().unwrap(), None, Some(json!({"message":"secret-unstructured-sentinel"})), cx);
        assert!(s.active_slash_commands().is_none());
        assert_eq!(s.workspaces[0].slash_catalogs[&Some("parent".into())].error.as_deref(), Some(SLASH_CATALOG_ERROR));
        assert!(!format!("{:?}{:?}{}", s.errors, s.log, s.workspaces[0].status).contains("secret-unstructured-sentinel"));
        s.ensure_slash_catalog(false, cx);
        assert!(rx.try_recv().is_err());
    });
}

#[gpui::test]
fn slash_catalog_rejects_wrong_scope_and_retires_reconnect_reads(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        s.workspaces[0].started = true;
        s.workspaces[0].inbound = Some(tx);
        s.active_workspace = Some(key.clone());
        s.conversations
            .entry("parent".into())
            .or_default()
            .subscribed = true;
        for session in [None, Some("parent")] {
            s.active = session.map(str::to_owned);
            s.ensure_slash_catalog(true, cx);
            let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
            let mut value = response(s, session, json!([]));
            if session.is_some() {
                value["session"]["sessionId"] = json!("wrong");
            } else {
                value["workspace"]["workspaceKey"] = json!("wrong");
            }
            s.handle_response(&key, request["id"].as_u64().unwrap(), Some(value), None, cx);
            assert!(s.active_slash_commands().is_none());
        }
        for missing in [
            json!({"session":{"sessionId":"parent"}}),
            response(s, Some("parent"), json!(null)),
        ] {
            s.ensure_slash_catalog(true, cx);
            let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
            s.handle_response(
                &key,
                request["id"].as_u64().unwrap(),
                Some(missing),
                None,
                cx,
            );
            assert!(s.active_slash_commands().is_none());
        }
        s.ensure_slash_catalog(true, cx);
        let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        let value = response(
            s,
            Some("parent"),
            crate::composer::slash_catalog::tests::commands(),
        );
        s.workspaces[0].invalidate_connection();
        s.handle_response(&key, request["id"].as_u64().unwrap(), Some(value), None, cx);
        assert!(s.workspaces[0].slash_catalogs.is_empty());
        assert!(s.workspaces[0].pending.is_empty());
        drop(rx);
        s.ensure_slash_catalog(true, cx);
        assert!(!s.active_slash_query().unwrap().loading);
        assert!(s.active_slash_commands().is_none());
        assert!(s.workspaces[0].pending.is_empty());
        assert!(parse_commands(&json!([])).unwrap().is_empty());
    });
}
