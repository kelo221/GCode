use crate::app::store::AppState;
use crate::conversation::model::ConversationState;
use crate::conversation::subagents::{RunningSubagent, SubagentsState};
use gpui::{AppContext, TestAppContext};

fn parent(child: &str) -> ConversationState {
    ConversationState {
        subscribed: true,
        subagents: Some(SubagentsState {
            child_session_ids: vec![child.into()],
            running: vec![RunningSubagent {
                child_session_id: child.into(),
                title: "Research".into(),
                status: "running".into(),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[gpui::test]
fn child_navigation_rejects_unknown_and_conflicting_owner(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        s.active_workspace = Some(key.clone());
        s.active = Some("parent".into());
        s.conversations.insert("parent".into(), parent("child"));
        s.open_subagent("unknown", cx);
        assert!(s.viewing_child.is_none());
        assert!(!s.child_owner.contains_key("unknown"));
        s.child_owner
            .insert("child".into(), (key, "other-parent".into()));
        s.open_subagent("child", cx);
        assert!(s.viewing_child.is_none());
        assert_eq!(s.child_owner["child"].1, "other-parent");
    });
}

#[gpui::test]
fn switching_child_releases_only_previous_child(cx: &mut TestAppContext) {
    use crate::backend::workspace::RouteSubscription;
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        s.active_workspace = Some(key.clone());
        s.active = Some("parent".into());
        let mut conv = parent("child-a");
        conv.subagents
            .as_mut()
            .unwrap()
            .child_session_ids
            .push("child-b".into());
        s.conversations.insert("parent".into(), conv);
        for sid in ["parent", "child-a", "child-b"] {
            s.workspaces[0].subscriptions.insert(
                format!("conversation/{sid}"),
                RouteSubscription {
                    id: sid.into(),
                    log_epoch: "epoch".into(),
                },
            );
        }
        for sid in ["child-a", "child-b"] {
            s.conversations.insert(
                sid.into(),
                ConversationState {
                    subscribed: true,
                    ..Default::default()
                },
            );
        }
        s.open_subagent("child-a", cx);
        s.open_subagent("child-b", cx);
        assert_eq!(s.viewing_child.as_deref(), Some("child-b"));
        assert!(!s.conversations.contains_key("child-a"));
        assert!(
            !s.workspaces[0]
                .subscriptions
                .contains_key("conversation/child-a")
        );
        assert!(
            s.workspaces[0]
                .subscriptions
                .contains_key("conversation/parent")
        );
        s.close_subagent(cx);
        assert!(s.conversations["parent"].subscribed);
        assert!(
            s.workspaces[0]
                .subscriptions
                .contains_key("conversation/parent")
        );
    });
}

#[gpui::test]
fn nested_explore_opens_under_its_subagent_and_back_goes_up_one_level(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        s.active_workspace = Some(key.clone());
        s.active = Some("parent".into());
        s.conversations.insert("parent".into(), parent("x"));
        s.conversations.insert("x".into(), parent("g"));
        assert_eq!(
            s.subagent_open_owner(),
            Some((key.clone(), "parent".into()))
        );

        s.open_subagent_for(&key, "parent", "x", cx);
        assert_eq!(s.viewing_child.as_deref(), Some("x"));
        // Inside X, Open buttons are owned by X.
        assert_eq!(s.subagent_open_owner(), Some((key.clone(), "x".into())));

        // A parent that is not the active session or one of its subagents is refused.
        s.open_subagent_for(&key, "stranger", "g", cx);
        assert_eq!(s.viewing_child.as_deref(), Some("x"));

        s.open_subagent_for(&key, "x", "g", cx);
        assert_eq!(s.viewing_child.as_deref(), Some("g"));
        assert_eq!(s.parent_of_viewing_child().as_deref(), Some("x"));
        assert_eq!(s.known_subagent_title("x").as_deref(), Some("Research"));

        s.close_subagent(cx);
        assert_eq!(s.viewing_child.as_deref(), Some("x"));
        s.close_subagent(cx);
        assert!(s.viewing_child.is_none());
    });
}

#[gpui::test]
fn nested_explore_opens_from_main_view_via_the_nested_directory(cx: &mut TestAppContext) {
    use crate::conversation::subagent_directory::DirectoryAgent;
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        s.active_workspace = Some(key.clone());
        s.active = Some("parent".into());
        s.conversations.insert("parent".into(), parent("x"));
        // Main view: G is only known from the polled directory of X.
        s.workspaces[0]
            .subagent_directories
            .entry("x".into())
            .or_default()
            .running
            .push(DirectoryAgent {
                child_session_id: "g".into(),
                agent_id: None,
                tool_call_id: None,
                subagent_type: "Explore".into(),
                title: "Find".into(),
                summary: None,
                status: "running".into(),
                started_at: Some(1),
                ended_at: None,
            });
        s.open_subagent_for(&key, "x", "g", cx);
        assert_eq!(s.viewing_child.as_deref(), Some("g"));
        assert_eq!(s.parent_of_viewing_child().as_deref(), Some("x"));
        s.close_subagent(cx);
        assert_eq!(s.viewing_child.as_deref(), Some("x"));
    });
}

#[test]
fn background_counts_join_subagents_without_double_counting() {
    use crate::app::activity::ActivityCounts;
    use crate::conversation::subagents::BackgroundWork;
    let mut conv = parent("child");
    conv.background_works = vec![
        BackgroundWork {
            kind: "subagent".into(),
            status: "running".into(),
            child_session_id: Some("child".into()),
            ..Default::default()
        },
        BackgroundWork {
            kind: "bash".into(),
            status: "running".into(),
            ..Default::default()
        },
        BackgroundWork {
            kind: "workflow".into(),
            status: "running".into(),
            ..Default::default()
        },
        BackgroundWork {
            kind: "bash".into(),
            status: "completed".into(),
            ..Default::default()
        },
    ];
    assert_eq!(
        ActivityCounts::from_conversation(&conv),
        ActivityCounts {
            subagents: 1,
            bash: 1,
            workflows: 1
        }
    );
    let counts = ActivityCounts::from_conversation(&conv);
    assert_eq!(counts.label(true), "3");
    assert!(counts.label(false).contains("1"));
    assert_eq!(counts.label(false).split(" · ").count(), 3);
    assert_eq!(
        ActivityCounts::from_conversation(&ConversationState::default()).total(),
        0
    );
}

#[gpui::test]
fn cancellation_targets_work_and_suppresses_pending_duplicates(cx: &mut TestAppContext) {
    use crate::conversation::subagents::BackgroundWork;
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let key = s.workspaces[0].key.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        s.workspaces[0].inbound = Some(tx);
        s.workspaces[0].started = true;
        s.active_workspace = Some(key.clone());
        s.active = Some("parent".into());
        let mut conv = parent("child");
        conv.background_works.push(BackgroundWork {
            work_id: "run-id".into(),
            kind: "workflow".into(),
            status: "running".into(),
            ..Default::default()
        });
        s.conversations.insert("parent".into(), conv);
        s.cancel_background_work("run-id", cx);
        s.cancel_background_work("run-id", cx);
        let command: serde_json::Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(command["params"]["type"], "cancelBackgroundWork");
        assert_eq!(command["params"]["payload"]["workId"], "run-id");
        assert!(rx.try_recv().is_err());
        let id = command["id"].as_u64().unwrap();
        s.handle_response(
            &key,
            id,
            Some(serde_json::json!({"status":"rejected", "reasonCode":"cancelUnsupported"})),
            None,
            cx,
        );
        assert_eq!(
            s.conversations["parent"].background_works[0].status,
            "running"
        );
        assert!(
            s.errors
                .iter()
                .any(|error| error.contains("cancelUnsupported"))
        );
    });
}

#[test]
fn composer_activity_pointer_child_and_back_preserve_parent_input() {
    let _guard = crate::app::test_support::RUNTIME_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    use crate::app::{root::RootView, test_support::TestTargets};
    use crate::shared::{preferences::Preferences, settings::AppSettings};
    use gpui::{MouseButton, TestApp, px, size};
    use std::sync::Arc;
    let mut app = TestApp::with_text_system_and_assets(
        Arc::new(gpui::NoopTextSystem::new()),
        Arc::new(ely_gpui_component::Assets),
    );
    app.update(|cx| {
        ely_gpui_component::init(cx);
        cx.set_global(TestTargets::default());
        Preferences::install_at(
            AppSettings::default(),
            std::env::temp_dir().join(uuid::Uuid::now_v7().to_string()),
            Arc::new(|| true),
            cx,
        );
    });
    let state = app.new_entity(AppState::for_test);
    let attachment = crate::composer::attachment::AttachmentRef {
        reference: "C:/fixtures/note.txt".into(),
        file_name: "note.txt".into(),
        mime: "text/plain".into(),
        bytes: 1,
        preview_ref: None,
    };
    app.update_entity(&state, |s, cx| {
        s.active_workspace = Some(s.workspaces[0].key.clone());
        s.active = Some("parent".into());
        let mut conv = parent("child");
        // A title far wider than the side panel: Stop / Open must still fit inside it.
        conv.subagents.as_mut().unwrap().running[0].title =
            "Port stb_vorbis to Rust bit-exact with every decoder path verified".into();
        conv.subagents.as_mut().unwrap().running[0].started_at = Some(0);
        conv.subagents.as_mut().unwrap().running[0].model = Some("zai/glm-4.6".into());
        conv.subagents.as_mut().unwrap().running[0].todos =
            vec![crate::conversation::turn_meta::PlanItem {
                id: "a".into(),
                content: "Read pane".into(),
                status: "inProgress".into(),
            }];
        s.conversations.insert("parent".into(), conv);
        s.composer.update(cx, |c, cx| {
            c.set_text("parent draft");
            c.add_attachment(attachment.clone(), cx);
        });
    });
    let mut window = app.open_window(|window, cx| {
        let focus = state.read(cx).composer.read(cx).focus.clone();
        window.focus(&focus, cx);
        RootView::new(state.clone(), cx)
    });
    // 767 px and below switch to the phone layout; 780 is the narrowest desktop window.
    for (width, compact) in [(1280., false), (780., true), (1280., false)] {
        let requested = size(px(width), px(820.));
        window.simulate_resize(requested);
        // 测试平台的同步 resize 回调借用冲突；显式同步 viewport，避免用旧宽度验证响应布局。
        window.update(|_, window, cx| {
            window.bounds_changed(cx);
            assert_eq!(window.viewport_size(), requested);
        });
        window.draw();
        window.draw();
        window.read(|v, _| assert_eq!(v.composer_compact, compact));
    }
    let trigger = app.update(|cx| {
        cx.global::<TestTargets>()
            .0
            .get("composer-activity")
            .copied()
            .expect("composer activity entry")
    });
    window.simulate_click(trigger.center(), MouseButton::Left);
    window.read(|v, cx| {
        assert!(v.dock_open && v.agents_expanded);
        assert!(v.dock_tab == crate::app::dock::DockTab::Review);
        assert_eq!(v.state.read(cx).composer.read(cx).text(), "parent draft");
    });
    window.draw();
    let open = app.update(|cx| cx.global::<TestTargets>().0["agent-open-child"]);
    app.update(|cx| {
        let targets = &cx.global::<TestTargets>().0;
        assert!(
            targets.contains_key("agent-model-child"),
            "card shows the child's model"
        );
        assert!(
            targets.contains_key("agent-todo-child"),
            "card shows the child's todo"
        );
    });
    let dock_right = 1280. - crate::app::layout::PANEL_GAP;
    assert!(
        f32::from(open.right()) <= dock_right,
        "Open button overflows the side panel: {open:?}"
    );
    window.simulate_click(open.center(), MouseButton::Left);
    window.read(|v, cx| {
        assert_eq!(v.state.read(cx).viewing_child.as_deref(), Some("child"));
        assert!(
            v.dock_open,
            "opening a subagent must not close the side panel"
        );
    });
    window.simulate_input("must not reach parent");
    window.simulate_keystroke("enter");
    window.read(|v, cx| {
        let s = v.state.read(cx);
        assert_eq!(s.composer.read(cx).text(), "parent draft");
        assert_eq!(
            s.composer.read(cx).attachments(),
            std::slice::from_ref(&attachment)
        );
        assert!(s.conversations["parent"].subscribed);
    });
    window.update(|v, window, cx| {
        v.close_child_conversation(window, cx);
        assert!(v.state.read(cx).composer.read(cx).focus.is_focused(window));
    });
    window.read(|v, cx| {
        assert!(v.state.read(cx).viewing_child.is_none());
        assert!(
            v.dock_open,
            "returning to the parent keeps the side panel open"
        );
    });
}
