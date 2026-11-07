use crate::app::store::AppState;
use crate::composer::delivery::{InputRouting, SubmitTrigger};
use crate::conversation::model::ConversationState;
use gpui::{AppContext, TestAppContext};
use serde_json::{Value, json};

fn setup(s: &mut AppState, parent: bool) -> std::sync::mpsc::Receiver<String> {
    let key = s.workspaces[0].key.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    s.workspaces[0].started = true;
    s.workspaces[0].pumping = true;
    s.workspaces[0].inbound = Some(tx);
    s.active_workspace = Some(key);
    s.active = parent.then(|| "parent".into());
    s.draft = !parent;
    if parent {
        let mut conv = ConversationState {
            subscribed: true,
            ..Default::default()
        };
        conv.apply_snapshot(&json!({"revision":1,"config":{"mode":"edit","followupMode":"queue"},"inputRouting":{"mode":"enqueue"}}));
        s.conversations.insert("parent".into(), conv);
    }
    rx
}

#[gpui::test]
fn bare_plan_edits_one_draft_owner_without_command_or_live_mode(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        for parent in [false, true] {
            let rx = setup(s, parent);
            s.composer.update(cx, |c, _| c.set_text("/plan"));
            s.submit_composer(cx);
            assert!(s.composer.read(cx).text().is_empty());
            assert_eq!(s.submission_override()["planEnabled"], true);
            assert!(s.effective_config().plan_enabled);
            assert_ne!(s.effective_config().mode, "plan");
            if parent {
                assert_eq!(s.conversations["parent"].config.mode, "edit");
            }
            assert!(rx.try_recv().is_err());
            assert!(s.workspaces[0].pending.is_empty());
            assert!(s.held_confirmation.is_none());
            // Picking a permission keeps the plan overlay; only the check turns it off.
            assert!(s.set_restored_mode("yolo"));
            assert_eq!(s.submission_override()["planEnabled"], true);
            assert_eq!(s.effective_config().mode, "yolo");
            s.set_plan_enabled(false, cx);
            assert_eq!(s.submission_override()["planEnabled"], false);
            assert_eq!(s.submission_override()["mode"], "yolo");
            assert!(rx.try_recv().is_err());
            s.composer.update(cx, |c, _| c.set_text("/plan"));
            s.workspaces[0].started = false;
            s.submit_composer(cx);
            assert!(s.composer.read(cx).text().is_empty());
            assert_eq!(s.submission_override()["planEnabled"], true);
            assert!(rx.try_recv().is_err());
        }
    });
}

#[gpui::test]
fn plan_task_uses_ordinary_nonrecursive_input_and_recovers_original_text(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let rx = setup(s, true);
        let key = s.workspaces[0].key.clone();
        s.draft_submission_overrides.insert("parent".into(), json!({"mode":"edit","modelSelection":{"providerId":"test","modelId":"model","options":{"reasoningLevel":"high"}}}));
        s.composer.update(cx, |c, _| c.set_text(" /PLAN /compact\n  keep spacing "));
        s.submit_composer_with_trigger(SubmitTrigger::ModifiedEnter, cx);
        let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(request["params"]["type"], "sendText");
        let payload = &request["params"]["payload"];
        assert_eq!(payload["text"], "/compact\n  keep spacing");
        assert_eq!(payload["mode"], "edit");
        assert_eq!(payload["planEnabled"], true);
        assert_eq!(payload["requestedDelivery"], "startNow");
        assert_eq!(payload["modelSelection"]["options"]["reasoningLevel"], "high");
        assert!(s.submission_override().is_null());
        s.composer.update(cx, |c, _| c.set_text("newer"));
        s.handle_response(&key, request["id"].as_u64().unwrap(), Some(json!({"status":"rejected"})), None, cx);
        assert_eq!(s.composer.read(cx).text(), "/PLAN /compact\n  keep spacing\nnewer");
        assert_eq!(s.submission_override()["planEnabled"], true);
        assert_eq!(s.conversations["parent"].config.mode, "edit");
        assert!(rx.try_recv().is_err());
    });
}

#[gpui::test]
fn plan_new_first_input_and_config_share_frozen_override(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let rx = setup(s, false);
        s.ui_mode = Some("yolo".into());
        s.ui_plan = true;
        s.composer.update(cx, |c, _| c.set_text("/plan new task"));
        s.submit_composer(cx);
        let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(request["params"]["type"], "createSession");
        for target in ["firstInput", "config"] {
            // Full access + Plan keeps Full access underneath the overlay.
            assert_eq!(request["params"]["payload"][target]["mode"], "yolo");
            assert_eq!(request["params"]["payload"][target]["planEnabled"], true);
        }
        assert_eq!(
            request["params"]["payload"]["firstInput"]["text"],
            "new task"
        );
        assert!(rx.try_recv().is_err());
    });
}

#[gpui::test]
fn plan_payload_and_read_only_gates_preserve_text_chips_ownership_and_config(
    cx: &mut TestAppContext,
) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let rx = setup(s, true);
        for invalid in 0..3 {
            s.viewing_child = None;
            s.composer.update(cx, |c, _| {
                c.set_text("/plan task");
                c.attachments.clear();
                match invalid {
                    0 => c
                        .attachments
                        .push(crate::composer::attachment::AttachmentRef {
                            reference: "image".into(),
                            file_name: "image.png".into(),
                            mime: "image/png".into(),
                            bytes: 1,
                            preview_ref: None,
                        }),
                    1 => c.chips.insert(6..10),
                    _ => c.marked_utf16 = Some(0..5),
                }
                c.temp_owned = vec![std::path::PathBuf::from("owned-paste")];
            });
            let replacement = s.composer.read(cx).replacement_generation;
            let chips = s.composer.read(cx).chips.clone();
            let attachments = s.composer.read(cx).attachments.clone();
            s.submit_composer(cx);
            assert_eq!(s.composer.read(cx).text(), "/plan task");
            assert_eq!(s.composer.read(cx).replacement_generation, replacement);
            assert_eq!(s.composer.read(cx).attachments, attachments);
            assert_eq!(s.composer.read(cx).chips, chips);
            assert_eq!(
                s.composer.read(cx).temp_owned,
                vec![std::path::PathBuf::from("owned-paste")]
            );
            assert!(s.submission_override().is_null());
            assert!(rx.try_recv().is_err());
        }
        s.composer.update(cx, |c, _| {
            c.set_text("/plan task");
            c.attachments.clear();
        });
        s.viewing_child = Some("child".into());
        s.submit_composer(cx);
        assert_eq!(s.composer.read(cx).text(), "/plan task");
        assert!(s.submission_override().is_null());
        assert!(rx.try_recv().is_err());
        s.viewing_child = None;
        s.composer_intent = crate::conversation::msg_actions::ComposerIntent::Rename {
            ws_key: s.workspaces[0].key.clone(),
            sid: "parent".into(),
        };
        s.submit_composer(cx);
        let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(request["params"]["type"], "renameSession");
        assert!(s.submission_override().is_null());
    });
}

#[gpui::test]
fn plan_held_queue_requires_review_and_stale_guard_restores_shortcut(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let rx = setup(s, true);
        let key = s.workspaces[0].key.clone();
        let conv = s.conversations.get_mut("parent").unwrap();
        conv.input_routing = Some(InputRouting::Choice);
        conv.queue = crate::conversation::queue::QueueState::from_value(
            &json!({"items":[{"queueItemId":"held","text":"old"}]}),
        );
        s.composer.update(cx, |c, _| c.set_text("/plan task"));
        s.submit_composer(cx);
        let receipt = s.held_confirmation.clone().unwrap();
        assert_eq!(s.composer.read(cx).text(), "/plan task");
        assert!(rx.try_recv().is_err());
        s.confirm_held_submission(
            receipt,
            crate::composer::held_queue::HeldDisposition::Keep,
            cx,
        );
        let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(request["params"]["payload"]["text"], "task");
        assert_eq!(request["params"]["payload"]["planEnabled"], true);
        assert_eq!(
            request["params"]["payload"]["heldQueueDisposition"],
            "keepQueueAndSend"
        );
        s.handle_response(
            &key,
            request["id"].as_u64().unwrap(),
            Some(json!({"status":"failed","reasonCode":"guard.heldQueueConfirmationStale"})),
            None,
            cx,
        );
        assert_eq!(s.composer.read(cx).text(), "/plan task");
        assert!(s.held_confirmation.is_some());
        assert!(rx.try_recv().is_err());
    });
}
