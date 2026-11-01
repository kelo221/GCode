use super::*;
use crate::app::{root::RootView, store::AppState, test_support::*};
use crate::conversation::model::ConversationState;
use crate::conversation::subagents::{RunningSubagent, SubagentsState};
use crate::shared::preferences::Preferences;
use crate::shared::settings::AppSettings;
use gpui::{MouseButton, TestApp, px, size};
use std::sync::Arc;

fn running(child: &str, started: u64) -> JoinedRunningSubagent {
    JoinedRunningSubagent {
        child_session_id: child.into(),
        agent_id: None,
        tool_call_id: None,
        subagent_type: "general-purpose".into(),
        title: format!("Run {child}"),
        summary: None,
        status: "running".into(),
        started_at: Some(started),
        model: None,
        todos: vec![],
        work_id: None,
        cancellable: false,
    }
}

fn ended(child: &str, started: u64, status: &str) -> DirectoryAgent {
    DirectoryAgent {
        child_session_id: child.into(),
        agent_id: None,
        tool_call_id: None,
        subagent_type: "Explore".into(),
        title: format!("Done {child}"),
        summary: None,
        status: status.into(),
        started_at: Some(started),
        ended_at: Some(started + 5),
    }
}

fn numbers(agents: &[HeaderAgent]) -> Vec<(usize, &str)> {
    agents
        .iter()
        .map(|a| (a.number, a.child.as_str()))
        .collect()
}

#[test]
fn numbers_follow_start_order_across_running_and_ended() {
    let agents = header_agents(
        &[running("c", 30), running("a", 10)],
        &[ended("b", 20, "success")],
        None,
    );
    assert_eq!(numbers(&agents.shown), vec![(1, "a"), (2, "b"), (3, "c")]);
    assert!(agents.overflow.is_empty());
    assert_eq!(agents.shown[1].dot, AgentDot::None);
    assert_eq!(agents.shown[0].dot, AgentDot::Running);
}

#[test]
fn live_running_row_wins_over_a_stale_directory_row() {
    let agents = header_agents(&[running("a", 10)], &[ended("a", 10, "success")], None);
    assert_eq!(agents.shown.len(), 1);
    assert_eq!(agents.shown[0].dot, AgentDot::Running);
}

#[test]
fn overflow_keeps_open_child_and_running_agents_then_newest_ended() {
    let ended_rows: Vec<_> = (1..=6)
        .map(|n| {
            ended(
                &format!("e{n}"),
                n * 10,
                if n == 2 { "failed" } else { "success" },
            )
        })
        .collect();
    // Started last but running; e1 is the open child.
    let agents = header_agents(&[running("r", 100)], &ended_rows, Some("e1"));
    assert_eq!(
        numbers(&agents.shown),
        vec![(1, "e1"), (5, "e5"), (6, "e6"), (7, "r")]
    );
    assert!(agents.shown[0].viewing);
    assert_eq!(
        numbers(&agents.overflow),
        vec![(2, "e2"), (3, "e3"), (4, "e4")]
    );
    assert_eq!(agents.overflow[0].dot, AgentDot::Failed);
}

#[test]
fn blank_titles_fall_back_to_the_agent_type() {
    let mut row = running("a", 1);
    row.title = "  ".into();
    let agents = header_agents(&[row], &[], None);
    assert_eq!(agents.shown[0].title, "general-purpose");
}

fn app() -> TestApp {
    let mut app = TestApp::with_text_system_and_assets(
        Arc::new(gpui::NoopTextSystem::new()),
        Arc::new(ely_gpui_component::Assets),
    );
    app.update(|cx| {
        ely_gpui_component::init(cx);
        cx.set_global(TestTargets::default());
        Preferences::install_at(
            AppSettings::default(),
            std::env::temp_dir()
                .join(format!("gpui-mobile-agents-{}", uuid::Uuid::now_v7()))
                .join("setting.json"),
            Arc::new(|| false),
            cx,
        );
    });
    app
}

fn target(app: &mut TestApp, id: &str) -> Option<gpui::Bounds<gpui::Pixels>> {
    app.read(|cx| cx.global::<TestTargets>().0.get(id).copied())
}

#[test]
fn phone_header_icon_opens_the_child_and_tapping_it_again_returns() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = app();
    let state = app.new_entity(AppState::for_test);
    app.update_entity(&state, |s, _| {
        s.active_workspace = Some(s.workspaces[0].key.clone());
        s.active = Some("parent".into());
        s.conversations.insert(
            "parent".into(),
            ConversationState {
                subscribed: true,
                subagents: Some(SubagentsState {
                    child_session_ids: vec!["child".into()],
                    running: vec![RunningSubagent {
                        child_session_id: "child".into(),
                        title: "Research".into(),
                        status: "running".into(),
                        started_at: Some(1),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
    });
    let mut window = app.open_window(|_, cx| RootView::new(state.clone(), cx));
    window.simulate_resize(size(px(411.), px(800.)));
    window.update(|view, window, cx| {
        window.bounds_changed(cx);
        view.show_chat_page();
    });
    window.draw();
    let icon = target(&mut app, "mobile-agent-1").expect("header shows agent 1");
    assert!(target(&mut app, "mobile-agent-more").is_none());

    window.simulate_click(icon.center(), MouseButton::Left);
    window.read(|v, cx| assert_eq!(v.state.read(cx).viewing_child.as_deref(), Some("child")));

    // The icons still describe the parent while its child is open.
    app.update(|cx| cx.global_mut::<TestTargets>().0.clear());
    window.draw();
    let icon = target(&mut app, "mobile-agent-1").expect("icon stays while viewing the child");
    window.simulate_click(icon.center(), MouseButton::Left);
    window.read(|v, cx| assert!(v.state.read(cx).viewing_child.is_none()));
}

#[test]
fn phone_header_has_no_icons_without_subagents() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = app();
    let state = app.new_entity(AppState::for_test);
    let mut window = app.open_window(|_, cx| RootView::new(state.clone(), cx));
    window.simulate_resize(size(px(411.), px(800.)));
    window.update(|view, window, cx| {
        window.bounds_changed(cx);
        view.show_chat_page();
    });
    window.draw();
    assert!(target(&mut app, "mobile-back-home").is_some());
    assert!(target(&mut app, "mobile-agent-1").is_none());
}
