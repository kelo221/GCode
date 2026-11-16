//! Golden tests for `conversation/subagent_stop.rs`: `stopControl` parsing,
//! command choice (stopSubagent vs cancelBackgroundWork) and the rejection
//! banner.

use super::*;
use crate::app::store::AppState;
use crate::conversation::subagents::{BackgroundWork, RunningSubagent, SubagentsState};
use gpui::{AppContext, TestAppContext};

fn control(task: &str, exec: &str) -> SubagentStopControl {
    SubagentStopControl {
        task_id: task.into(),
        execution_id: exec.into(),
    }
}

#[test]
fn parses_stop_control_from_running_summary() {
    let v = json!({
        "childSessionId": "sess_subagent_agent_1", "subagentType": "Explore",
        "title": "Find", "status": "running",
        "stopControl": { "taskId": "task-1", "executionId": "exec-1" }
    });
    let sub = RunningSubagent::from_value(&v).unwrap();
    assert_eq!(sub.stop_control, Some(control("task-1", "exec-1")));
    // Older CLIs omit it; a partial or empty control is not a stop handle.
    for bad in [
        json!(null),
        json!({ "taskId": "task-1" }),
        json!({ "taskId": "", "executionId": "exec-1" }),
        json!("task-1"),
    ] {
        let mut v = v.clone();
        v["stopControl"] = bad;
        assert_eq!(RunningSubagent::from_value(&v).unwrap().stop_control, None);
    }
}

#[test]
fn stop_control_wins_over_background_work() {
    let c = control("task-1", "exec-1");
    assert_eq!(
        stop_target(Some(&c), Some("work-1"), true),
        Some(SubagentStop::Execution(c.clone()))
    );
    // Upstream offers Stop from stopControl alone, even without a work id.
    assert_eq!(
        stop_target(Some(&c), None, false),
        Some(SubagentStop::Execution(c))
    );
    assert_eq!(
        stop_target(None, Some("work-1"), true),
        Some(SubagentStop::BackgroundWork("work-1".into()))
    );
    assert_eq!(stop_target(None, Some("work-1"), false), None);
    assert_eq!(stop_target(None, Some(" "), true), None);
    assert_eq!(stop_target(None, None, true), None);
}

#[test]
fn command_shapes_match_the_protocol() {
    let (ctype, payload) = SubagentStop::Execution(control("t", "e")).command();
    assert_eq!(ctype, "stopSubagent");
    assert_eq!(payload, json!({ "taskId": "t", "executionId": "e" }));
    let (ctype, payload) = SubagentStop::BackgroundWork("w".into()).command();
    assert_eq!(ctype, "cancelBackgroundWork");
    assert_eq!(payload, json!({ "workId": "w" }));
}

fn conversation(stop_control: Option<SubagentStopControl>) -> ConversationState {
    ConversationState {
        subscribed: true,
        subagents: Some(SubagentsState {
            child_session_ids: vec!["child".into()],
            running: vec![RunningSubagent {
                child_session_id: "child".into(),
                status: "running".into(),
                stop_control,
                ..Default::default()
            }],
            ..Default::default()
        }),
        background_works: vec![BackgroundWork {
            work_id: "work-1".into(),
            kind: "subagent".into(),
            status: "running".into(),
            child_session_id: Some("child".into()),
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn transcript_row_target_uses_the_running_summary() {
    let c = control("task-1", "exec-1");
    assert_eq!(
        conversation(Some(c.clone())).subagent_stop_target(None, Some("child")),
        Some(SubagentStop::Execution(c))
    );
    assert_eq!(
        conversation(None).subagent_stop_target(None, Some("child")),
        Some(SubagentStop::BackgroundWork("work-1".into()))
    );
    let joined = conversation(Some(control("t", "e"))).running_subagents();
    assert_eq!(
        joined[0].stop_target(),
        Some(SubagentStop::Execution(control("t", "e")))
    );
}

#[gpui::test]
fn stop_sends_stop_subagent_once_and_explains_rejection(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        s.workspaces[0].inbound = Some(tx);
        s.workspaces[0].started = true;
        s.active_workspace = Some(key.clone());
        s.active = Some("parent".into());
        s.conversations.insert(
            "parent".into(),
            conversation(Some(control("task-1", "exec-1"))),
        );
        let stop = s.conversations["parent"]
            .subagent_stop_target(None, Some("child"))
            .unwrap();
        s.stop_subagent_for(&key, "parent", &stop, cx);
        s.stop_subagent_for(&key, "parent", &stop, cx);
        let command: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(command["params"]["type"], "stopSubagent");
        assert_eq!(
            command["params"]["payload"],
            json!({ "taskId": "task-1", "executionId": "exec-1" })
        );
        assert!(rx.try_recv().is_err(), "a pending stop is not resent");
        let id = command["id"].as_u64().unwrap();
        s.handle_response(
            &key,
            id,
            Some(json!({
                "status": "failed",
                "reasonCode": "fault.command.subagentStopRejected.not_running",
                "message": "subagent task-1 was not stopped: background_task_not_running"
            })),
            None,
            cx,
        );
        assert!(
            s.errors
                .iter()
                .any(|e| e.contains("Subagent not stopped") || e.contains("子代理未停止")),
            "errors: {:?}",
            s.errors
        );
    });
}
