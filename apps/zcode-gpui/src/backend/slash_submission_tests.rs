use crate::app::store::AppState;
use crate::composer::delivery::{InputRouting, SubmitTrigger};
use crate::conversation::model::ConversationState;
use gpui::{AppContext, TestAppContext};
use serde_json::{Value, json};

#[gpui::test]
fn init_first_input_and_rejected_plain_submission_keep_original_command(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        s.workspaces[0].started = true;
        s.workspaces[0].pumping = true;
        s.workspaces[0].inbound = Some(tx);
        s.active_workspace = Some(key.clone());
        s.composer
            .update(cx, |c, _| c.set_text("/init new-session notes"));
        s.submit_composer(cx);
        let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(request["params"]["type"], "createSession");
        assert_eq!(
            request["params"]["payload"]["firstInput"]["text"],
            "/init new-session notes"
        );
        assert!(
            request["params"]["payload"]
                .get("requestedDelivery")
                .is_none()
        );
        s.handle_response(
            &key,
            request["id"].as_u64().unwrap(),
            Some(json!({"status":"rejected"})),
            None,
            cx,
        );
        assert_eq!(s.composer.read(cx).text(), "/init new-session notes");
        s.active = Some("parent".into());
        s.draft = false;
        s.conversations.insert(
            "parent".into(),
            ConversationState {
                subscribed: true,
                input_routing: Some(InputRouting::Enqueue),
                ..Default::default()
            },
        );
        s.composer
            .update(cx, |c, _| c.set_text("/init parent notes"));
        s.submit_composer_with_trigger(SubmitTrigger::ModifiedEnter, cx);
        let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(request["params"]["type"], "sendText");
        assert_eq!(request["params"]["payload"]["text"], "/init parent notes");
        assert_eq!(
            request["params"]["payload"]["requestedDelivery"],
            "startNow"
        );
        s.composer.update(cx, |c, _| c.set_text("newer input"));
        s.handle_response(
            &key,
            request["id"].as_u64().unwrap(),
            Some(json!({"status":"rejected"})),
            None,
            cx,
        );
        assert_eq!(
            s.composer.read(cx).text(),
            "/init parent notes\nnewer input"
        );
        assert!(rx.try_recv().is_err());
    });
}

#[gpui::test]
fn late_slash_catalog_reply_populates_only_origin_scope_without_navigation(
    cx: &mut TestAppContext,
) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let origin = s.workspaces[0].key.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        s.workspaces[0].started = true;
        s.workspaces[0].inbound = Some(tx);
        s.active_workspace = Some(origin.clone());
        s.ensure_slash_catalog(false, cx);
        let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        let value = json!({"workspace":{"workspacePath":s.workspaces[0].path.to_string_lossy(),"workspaceKey":origin},"mode":"build","slashCommands":crate::composer::slash_catalog::tests::commands()});
        s.workspaces.push(crate::backend::workspace::WorkspaceHandle::new(std::env::temp_dir().join(uuid::Uuid::now_v7().to_string()), vec![]));
        let other = s.workspaces[1].key.clone();
        s.active_workspace = Some(other.clone());
        s.composer.update(cx, |c, _| c.set_text("keep other draft"));
        s.handle_response(&origin, request["id"].as_u64().unwrap(), Some(value), None, cx);
        assert_eq!(s.active_workspace.as_deref(), Some(other.as_str()));
        assert_eq!(s.composer.read(cx).text(), "keep other draft");
        assert!(s.active_slash_commands().is_none());
        assert!(s.workspaces[0].slash_catalogs[&None].value.is_some());
        assert!(s.workspaces[1].slash_catalogs.is_empty());
    });
}
