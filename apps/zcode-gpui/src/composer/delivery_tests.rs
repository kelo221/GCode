use crate::app::store::AppState;
use crate::composer::attachment::AttachmentRef;
use crate::composer::delivery::{InputRouting, SubmitTrigger};
use crate::conversation::model::ConversationState;
use gpui::{AppContext, TestAppContext};
use serde_json::{Value, json};

fn attachment(reference: &str) -> AttachmentRef {
    AttachmentRef {
        reference: reference.into(),
        file_name: "image.png".into(),
        mime: "image/png".into(),
        bytes: 1,
        preview_ref: None,
    }
}

#[gpui::test]
fn one_shot_delivery_does_not_change_followup_preference(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        s.workspaces[0].started = true;
        s.workspaces[0].pumping = true;
        s.workspaces[0].inbound = Some(tx);
        s.active_workspace = Some(key.clone());
        s.active = Some("session".into());
        s.draft = false;
        let mut conv = ConversationState {
            subscribed: true,
            ..Default::default()
        };
        conv.apply_snapshot(&json!({"control":{"phase":"running","canStop":true},"inputRouting":{"mode":"enqueue"},"config":{"followupMode":"queue"}}));
        s.conversations.insert("session".into(), conv);
        for (mode, trigger, expected) in [
            ("queue", SubmitTrigger::Ordinary, Value::Null),
            ("queue", SubmitTrigger::ModifiedEnter, json!("startNow")),
            ("queue", SubmitTrigger::ModifiedPointer, json!("startNow")),
            ("guide", SubmitTrigger::Ordinary, json!("guide")),
            ("guide", SubmitTrigger::ModifiedEnter, json!("queue")),
            ("guide", SubmitTrigger::ModifiedPointer, json!("queue")),
        ] {
            s.conversations.get_mut("session").unwrap().config.followup_mode = mode.into();
            s.composer.update(cx, |c, _| c.set_text("one input"));
            s.submit_composer_with_trigger(trigger, cx);
            let command: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
            assert_eq!(command["params"]["type"], "sendText");
            assert_eq!(command["params"]["payload"]["requestedDelivery"], expected);
            assert_eq!(s.conversations["session"].config.followup_mode, mode);
            assert!(rx.try_recv().is_err());
        }
        let conv = s.conversations.get_mut("session").unwrap();
        conv.config.followup_mode = "queue".into();
        conv.can_stop = false;
        conv.input_routing = None;
        s.composer.update(cx, |c, _| c.set_text("ordinary pointer while idle"));
        s.submit_composer_with_trigger(SubmitTrigger::ModifiedPointer, cx);
        let command: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert!(command["params"]["payload"].get("requestedDelivery").is_none());
    });
}

#[gpui::test]
fn delivery_reject_choice_and_missing_modified_route_preserve_owned_draft(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        s.workspaces[0].started = true;
        s.workspaces[0].pumping = true;
        s.workspaces[0].inbound = Some(tx);
        s.active_workspace = Some(key);
        s.active = Some("session".into());
        s.draft = false;
        s.conversations.insert(
            "session".into(),
            ConversationState {
                subscribed: true,
                ..Default::default()
            },
        );
        let attachment = attachment("owned-image");
        let owned = std::env::temp_dir().join(uuid::Uuid::now_v7().to_string());
        s.composer.update(cx, |c, _| {
            c.attachments.push(attachment.clone());
            c.temp_owned.push(owned.clone());
        });
        for route in [Some(InputRouting::Reject), Some(InputRouting::Choice), None] {
            let conv = s.conversations.get_mut("session").unwrap();
            conv.input_routing = route;
            conv.can_stop = true;
            for trigger in [SubmitTrigger::ModifiedEnter, SubmitTrigger::ModifiedPointer] {
                s.composer.update(cx, |c, _| c.set_text("not admitted"));
                let generation = s.composer.read(cx).replacement_generation;
                s.submit_composer_with_trigger(trigger, cx);
                let composer = s.composer.read(cx);
                assert_eq!(composer.text(), "not admitted");
                assert_eq!(composer.attachments, vec![attachment.clone()]);
                assert_eq!(composer.temp_owned, vec![owned.clone()]);
                assert_eq!(composer.replacement_generation, generation);
                assert!(s.retired_temp_files.is_empty());
                assert!(s.workspaces[0].pending.is_empty());
                assert!(rx.try_recv().is_err());
            }
        }
        assert!(!s.errors.is_empty());
    });
}

#[gpui::test]
fn delivery_new_session_and_local_action_compatibility(cx: &mut TestAppContext) {
    use crate::conversation::msg_actions::ComposerIntent;
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        s.workspaces[0].started = true;
        s.workspaces[0].pumping = true;
        s.workspaces[0].inbound = Some(tx);
        s.active_workspace = Some(key.clone());
        s.active = None;
        s.draft = true;
        s.composer.update(cx, |c, _| c.set_text("new session"));
        s.submit_composer_with_trigger(SubmitTrigger::ModifiedEnter, cx);
        let command: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(command["params"]["type"], "createSession");
        assert_eq!(
            command["params"]["payload"]["firstInput"]["text"],
            "new session"
        );
        assert!(
            command["params"]["payload"]
                .get("requestedDelivery")
                .is_none()
        );
        assert!(
            command["params"]["payload"]["firstInput"]
                .get("requestedDelivery")
                .is_none()
        );
        s.active = Some("session".into());
        s.draft = false;
        let conv = ConversationState {
            subscribed: true,
            input_routing: Some(InputRouting::Reject),
            ..Default::default()
        };
        s.conversations.insert("session".into(), conv);
        for slash in ["/compact", "/goal objective", "/goal resume"] {
            assert_eq!(
                s.submit_delivery(SubmitTrigger::ModifiedEnter, slash),
                Ok(None)
            );
        }
        for intent in [
            ComposerIntent::Rename {
                ws_key: key.clone(),
                sid: "session".into(),
            },
            ComposerIntent::Edit {
                ws_key: key,
                sid: "session".into(),
                row_id: 1,
                entity_id: "row".into(),
            },
        ] {
            s.composer_intent = intent;
            assert_eq!(
                s.submit_delivery(SubmitTrigger::ModifiedEnter, "intent text"),
                Ok(None)
            );
        }
        assert!(rx.try_recv().is_err());
    });
}

#[test]
fn delivery_primary_modifier_is_platform_specific() {
    use gpui::Modifiers;
    assert_eq!(
        super::primary_modifier(&Modifiers {
            control: true,
            ..Default::default()
        }),
        !cfg!(target_os = "macos")
    );
    assert_eq!(
        super::primary_modifier(&Modifiers {
            platform: true,
            ..Default::default()
        }),
        cfg!(target_os = "macos")
    );
    assert!(!super::primary_modifier(&Modifiers {
        alt: true,
        shift: true,
        ..Default::default()
    }));
}

#[test]
fn delivery_snapshot_and_whole_key_patch_replace_can_stop_and_routing() {
    let mut conv = ConversationState::default();
    conv.apply_snapshot(
        &json!({"control":{"phase":"running","canStop":true},"inputRouting":{"mode":"guide"}}),
    );
    assert!(conv.can_stop);
    assert_eq!(conv.input_routing, Some(InputRouting::Guide));
    conv.apply_deltas(&[json!({"op":"state.updated","patch":{"control":{"phase":"completedSuccess"},"inputRouting":null}})]);
    assert!(!conv.can_stop);
    assert_eq!(conv.input_routing, None);
    conv.apply_snapshot(&json!({"inputRouting":{"mode":"reject"}}));
    assert_eq!(conv.input_routing, Some(InputRouting::Reject));
    conv.apply_snapshot(&json!({}));
    assert_eq!(conv.input_routing, None);
}

#[gpui::test]
fn failed_delivery_enqueue_and_rejected_promotion_recover_without_replay(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        s.workspaces[0].started = true;
        s.workspaces[0].pumping = true;
        s.workspaces[0].inbound = Some(tx);
        s.active_workspace = Some(key.clone());
        s.active = Some("session".into());
        s.draft = false;
        let mut conv = ConversationState {
            subscribed: true,
            ..Default::default()
        };
        conv.apply_snapshot(&json!({"control":{"canStop":true},"inputRouting":{"mode":"enqueue"},"config":{"followupMode":"queue"}}));
        s.conversations.insert("session".into(), conv);
        let original = attachment("original-image");
        let newer = attachment("newer-image");
        let retired = std::env::temp_dir().join(uuid::Uuid::now_v7().to_string());
        let current_owned = std::env::temp_dir().join(uuid::Uuid::now_v7().to_string());
        s.composer.update(cx, |c, _| {
            c.set_text("promotion draft");
            c.attachments.push(original.clone());
            c.temp_owned.push(retired.clone());
        });
        s.submit_composer_with_trigger(SubmitTrigger::ModifiedEnter, cx);
        let command: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(command["params"]["payload"]["attachments"], json!([original.clone()]));
        assert_eq!(s.retired_temp_files, vec![retired.clone()]);
        s.composer.update(cx, |c, _| {
            c.set_text("newer");
            c.attachments.push(newer.clone());
            c.temp_owned.push(current_owned.clone());
        });
        s.handle_response(&key, command["id"].as_u64().unwrap(), Some(json!({"status":"rejected"})), None, cx);
        assert_eq!(s.composer.read(cx).text(), "promotion draft\nnewer");
        assert_eq!(s.composer.read(cx).attachments, vec![newer.clone(), original.clone()]);
        assert_eq!(s.composer.read(cx).temp_owned, vec![current_owned.clone()]);
        assert_eq!(s.retired_temp_files, vec![retired.clone()]);
        assert!(rx.try_recv().is_err());
        drop(rx);
        s.submit_composer_with_trigger(SubmitTrigger::ModifiedEnter, cx);
        assert_eq!(s.composer.read(cx).text(), "promotion draft\nnewer");
        assert_eq!(s.composer.read(cx).attachments, vec![newer, original]);
        assert_eq!(s.composer.read(cx).temp_owned, vec![current_owned]);
        assert_eq!(s.retired_temp_files, vec![retired]);
        assert!(s.workspaces[0].pending.is_empty());
    });
}
