use crate::app::store::AppState;
use crate::composer::delivery::InputRouting;
use crate::conversation::model::ConversationState;
use gpui::{AppContext, TestAppContext};
use serde_json::{Value, json};

#[gpui::test]
fn plan_only_override_preserves_draft_model_selection_in_selectors(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        s.active_workspace = Some(key.clone());
        let mut catalog = crate::composer::catalog::WorkspaceConfig::default();
        catalog.apply_state(&json!({"configOptions":[{"id":"model","currentValue":"test/model","options":[{"value":"test/model","name":"Selected model"}]}]}));
        s.workspace_configs.insert(key, catalog);
        s.ui_model_value = Some("test/model".into());
        s.composer.update(cx, |c, _| c.set_text("/plan"));
        s.submit_composer(cx);
        let config = s.effective_config();
        assert_eq!(config.provider, "test");
        assert_eq!(config.model, "model");
        assert!(config.plan_enabled);
        assert_eq!(config.mode, "build");
        assert!(s.set_restored_model("test", "other", "high"));
        assert_eq!(s.effective_config().model, "other");
        assert_eq!(s.submission_override()["planEnabled"], true);
    });
}

#[gpui::test]
fn plan_failed_enqueue_and_late_rejection_do_not_lose_original_or_newer_override(
    cx: &mut TestAppContext,
) {
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
        s.conversations.insert(
            "parent".into(),
            ConversationState {
                subscribed: true,
                input_routing: Some(InputRouting::Enqueue),
                ..Default::default()
            },
        );
        s.composer.update(cx, |c, _| c.set_text("/plan task"));
        drop(rx);
        s.submit_composer(cx);
        assert_eq!(s.composer.read(cx).text(), "/plan task");
        assert_eq!(s.submission_override()["planEnabled"], true);
        assert!(s.workspaces[0].pending.is_empty());
        let (tx, rx) = std::sync::mpsc::channel();
        s.workspaces[0].inbound = Some(tx);
        s.submit_composer(cx);
        let request: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        s.draft_submission_overrides
            .insert("parent".into(), json!({"mode":"yolo","planEnabled":false}));
        s.handle_response(
            &key,
            request["id"].as_u64().unwrap(),
            Some(json!({"status":"rejected"})),
            None,
            cx,
        );
        assert_eq!(s.composer.read(cx).text(), "/plan task");
        assert_eq!(
            s.submission_override(),
            json!({"mode":"yolo","planEnabled":false})
        );
        assert!(rx.try_recv().is_err());
    });
}
