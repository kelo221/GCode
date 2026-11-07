use super::*;
use gpui::{AppContext, TestAppContext};

#[gpui::test]
fn queue_edit_waits_for_ack_and_restores_full_draft_without_config_command(
    cx: &mut TestAppContext,
) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        s.workspaces[0].inbound = Some(tx);
        s.workspaces[0].started = true;
        s.active_workspace = Some(key.clone());
        s.active = Some("parent".into());
        s.draft = false;
        let mut conv = crate::conversation::model::ConversationState {
            subscribed: true,
            ..Default::default()
        };
        conv.apply_snapshot(&json!({"revision":7,"availability":{"queueEdit":{"allowed":true}},"queue":{"items":[{"queueItemId":"item","kind":"sendText","text":"original","attachments":[{"ref":"attachment://owned","fileName":"note.txt","mime":"text/plain","bytes":3}],"modelSelection":{"providerId":"provider","modelId":"model","options":{"reasoningLevel":"high"}},"mode":"yolo","planEnabled":false,"dispatch":{"state":"queued"}}]}}));
        s.conversations.insert("parent".into(), conv);
        s.restore_queued_input(&key, "parent", "item", cx);
        let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(request["params"]["type"], "deleteQueueItem");
        assert_eq!(request["params"]["baseRevision"], 7);
        assert!(s.composer.read(cx).text().is_empty());
        assert_eq!(s.conversations["parent"].queue.as_ref().unwrap().items.len(), 1);
        s.handle_response(&key, request["id"].as_u64().unwrap(), Some(json!({"status":"accepted"})), None, cx);
        assert_eq!(s.composer.read(cx).text(), "original");
        assert_eq!(s.composer.read(cx).attachments()[0].reference, "attachment://owned");
        assert!(s.composer.read(cx).temp_owned.is_empty());
        assert_eq!(s.draft_submission_overrides["parent"]["mode"], "yolo");
        assert_eq!(s.effective_config().model, "model");
        assert!(rx.try_recv().is_err());
        s.submit_composer(cx);
        let resend: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(resend["params"]["type"], "sendText");
        assert_eq!(resend["params"]["payload"]["modelSelection"]["options"]["reasoningLevel"], "high");
        assert_eq!(resend["params"]["payload"]["mode"], "yolo");
        assert_eq!(resend["params"]["payload"]["attachments"][0]["ref"], "attachment://owned");
        assert!(!s.draft_submission_overrides.contains_key("parent"));
        s.handle_response(&key, resend["id"].as_u64().unwrap(), Some(json!({"status":"rejected"})), None, cx);
        assert_eq!(s.composer.read(cx).text(), "original");
        assert_eq!(s.draft_submission_overrides["parent"]["mode"], "yolo");
        assert!(s.composer.read(cx).temp_owned.is_empty());
    });
}

#[gpui::test]
fn unknown_and_nonadmitted_queue_deletes_never_restore(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        s.active_workspace = Some(key.clone());
        s.active = Some("parent".into());
        for status in ["noop", "stale", "failed", "unknown"] {
            let restore = QueueRestore {
                item_id: "item".into(),
                submission: Submission {
                    workspace: key.clone(),
                    draft_key: "parent".into(),
                    navigation_generation: 0,
                    text: "do not restore".into(),
                    attachments: vec![],
                    config: json!({}),
                    model_choice: None,
                },
            };
            s.settle_queue_edit(restore, Some(&json!({"status": status})), cx);
            assert!(s.composer.read(cx).text().is_empty());
            assert!(s.session_drafts.is_empty());
        }
    });
}

#[gpui::test]
fn queue_ack_cannot_replace_new_text_or_hidden_parent(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        s.active_workspace = Some(key.clone());
        s.active = Some("parent".into());
        let restore = QueueRestore {
            item_id: "item".into(),
            submission: Submission {
                workspace: key,
                draft_key: "parent".into(),
                navigation_generation: 0,
                text: "old".into(),
                attachments: vec![],
                config: json!({"mode":"edit"}),
                model_choice: None,
            },
        };
        s.composer.update(cx, |c, _| c.set_text("new"));
        s.draft_submission_overrides
            .insert("parent".into(), json!({"mode":"yolo"}));
        s.settle_queue_edit(restore.clone(), Some(&json!({"status":"duplicate"})), cx);
        assert_eq!(s.composer.read(cx).text(), "new");
        assert_eq!(s.session_drafts["parent"], "old");
        assert_eq!(s.draft_submission_overrides["parent"]["mode"], "yolo");
        s.composer.update(cx, |c, _| c.set_text(""));
        s.viewing_child = Some("child".into());
        s.settle_queue_edit(restore, Some(&json!({"status":"accepted"})), cx);
        assert!(s.composer.read(cx).text().is_empty());
    });
}

#[gpui::test]
fn rejection_preserves_queue_and_navigation_keeps_recovery_at_origin(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        let restore = QueueRestore {
            item_id: "item".into(),
            submission: Submission {
                workspace: key.clone(),
                draft_key: "parent".into(),
                navigation_generation: 0,
                text: "old".into(),
                attachments: vec![],
                config: json!({"mode":"edit"}),
                model_choice: None,
            },
        };
        s.active_workspace = Some(key);
        s.active = Some("parent".into());
        s.settle_queue_edit(restore.clone(), Some(&json!({"status":"rejected"})), cx);
        assert!(s.composer.read(cx).text().is_empty());
        s.active = Some("other".into());
        s.composer.update(cx, |c, _| c.set_text("new"));
        s.settle_queue_edit(restore, Some(&json!({"status":"accepted"})), cx);
        assert_eq!(s.composer.read(cx).text(), "new");
        assert_eq!(s.session_drafts["parent"], "old");
        assert_eq!(s.draft_submission_overrides["parent"]["mode"], "edit");
    });
}
