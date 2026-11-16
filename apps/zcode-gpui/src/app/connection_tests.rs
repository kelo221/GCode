use crate::app::store::AppState;
use crate::backend::backlog::{EventBacklog, push_event};
use crate::backend::conn::BackendExit;
use crate::backend::events::attach_pump;
use crate::backend::launcher::ConnEvent;
use gpui::{AppContext, TestAppContext};

#[gpui::test]
fn superseded_pump_drops_old_line_log_exit_and_flow_work(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    let (tx, rx) = futures::channel::mpsc::unbounded();
    let backlog = EventBacklog::new(1024 * 1024);
    let logs = EventBacklog::new(1024 * 1024);
    let key = cx.read_entity(&state, |s, _| s.workspaces[0].key.clone());
    state.update(cx, |s, cx| {
        attach_pump(cx, key.clone(), 0, backlog.clone(), logs.clone(), rx);
        s.workspaces[0].invalidate_connection();
        s.workspaces[0].started = true;
        s.workspaces[0].pumping = true;
        s.workspaces[0].status = "replacement".into();
    });
    let line = r#"{"method":"startup/storageState","params":{"phase":"ready"}}"#;
    assert!(push_event(
        &backlog,
        &tx,
        ConnEvent::Line(line.into()),
        line.len()
    ));
    assert!(push_event(&logs, &tx, ConnEvent::Log("old-log".into()), 7));
    tx.unbounded_send(ConnEvent::Exited(None)).unwrap();
    drop(tx);
    cx.run_until_parked();
    state.read_with(cx, |s, _| {
        assert_eq!(s.workspaces[0].status, "replacement");
        assert!(s.workspaces[0].pumping);
        assert_eq!(s.workspaces[0].restart_attempts, 0);
        assert!(s.flow_saturated.is_empty());
        assert!(s.log.is_empty());
    });
}

#[gpui::test]
fn scheduled_restart_cannot_respawn_after_unload_generation(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        s.workspaces[0].started = true;
        s.workspaces[0].pumping = true;
        s.handle_conn(&key, ConnEvent::Exited(None), cx);
        s.workspaces[0].invalidate_connection();
        s.workspaces[0].status = "idle replacement".into();
    });
    cx.run_until_parked();
    cx.read_entity(&state, |s, _| {
        assert_eq!(s.workspaces[0].status, "idle replacement");
        assert!(!s.workspaces[0].pumping);
    });
}

/// Upstream v3.18.2 notifications GPUI does not consume are ignored without
/// errors and without ending the pump.
#[gpui::test]
fn new_upstream_notifications_are_ignored(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        for line in [
            r#"{"method":"v4/conversation/contextCompositionChanged","params":{"sessionId":"s","revision":3}}"#,
            r#"{"method":"v4/telemetry/model-request-metrics","params":{"sessionId":"s","ttftMs":12}}"#,
            r#"{"method":"startup/pluginsDegraded","params":{"reason":"timeout"}}"#,
        ] {
            assert!(s.handle_conn(&key, ConnEvent::Line(line.into()), cx));
        }
        assert!(s.errors.is_empty());
    });
}

fn exit_banner_after(exit: Option<BackendExit>, cx: &mut TestAppContext) -> Vec<String> {
    let state = cx.new(AppState::for_test);
    let errors = state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        s.workspaces[0].started = true;
        s.workspaces[0].pumping = true;
        s.workspaces[0].status = "connected".into();
        s.handle_conn(&key, ConnEvent::Exited(exit), cx);
        // Stop the scheduled respawn from launching a real backend.
        s.workspaces[0].invalidate_connection();
        s.errors.iter().cloned().collect::<Vec<_>>()
    });
    cx.run_until_parked();
    errors
}

#[gpui::test]
fn unexpected_exit_after_startup_shows_a_banner(cx: &mut TestAppContext) {
    let oom = exit_banner_after(
        Some(BackendExit {
            code: Some(0xE000_0008_u32 as i32),
            killed_by_gpui: false,
        }),
        cx,
    );
    assert_eq!(oom.len(), 1);
    assert!(oom[0].contains("ran out of memory") || oom[0].contains("内存不足"));
    let killed = exit_banner_after(
        Some(BackendExit {
            code: Some(1),
            killed_by_gpui: true,
        }),
        cx,
    );
    assert!(killed.is_empty(), "GPUI-initiated kills are not crashes");
    assert!(exit_banner_after(None, cx).is_empty());
}
