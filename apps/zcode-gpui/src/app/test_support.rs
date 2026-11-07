use super::store::AppState;
use crate::backend::workspace::WorkspaceHandle;
use gpui::{AppContext, Context};

pub(crate) static RUNTIME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Default)]
pub(crate) struct TestTargets(pub std::collections::HashMap<String, gpui::Bounds<gpui::Pixels>>);
impl gpui::Global for TestTargets {}

#[derive(Default)]
pub(crate) struct TestLabels(pub std::collections::HashMap<String, String>);
impl gpui::Global for TestLabels {}

pub(crate) fn track_children(element: gpui::Div, ids: Vec<String>) -> gpui::Div {
    element.on_children_prepainted(move |bounds, _, cx| {
        if cx.has_global::<TestTargets>() {
            let targets = cx.global_mut::<TestTargets>();
            for (id, bounds) in ids.iter().zip(bounds) {
                targets.0.insert(id.clone(), bounds);
            }
        }
    })
}

impl AppState {
    pub(crate) fn new_chat(&mut self, cx: &mut Context<Self>) {
        self.save_current_draft(cx);
        self.clear_subagent_view();
        self.active = None;
        self.draft = true;
        self.ui_model_value = None;
        self.ui_mode = None;
        self.ui_plan = false;
        self.ui_thought = None;
        self.restore_draft(
            &format!("draft:{}", self.active_workspace.as_deref().unwrap_or("")),
            cx,
        );
        cx.notify();
    }

    pub(crate) fn for_test(cx: &mut Context<Self>) -> Self {
        Self {
            workspaces: vec![WorkspaceHandle::new(std::env::temp_dir(), vec![])],
            active_workspace: None,
            navigation_generation: 0,
            plugin_prompt_applied: 0,
            held_confirmation: None,
            waking_send: None,
            conversations: Default::default(),
            workflow_navigation_generation: 0,
            workflow_focus: None,
            workflow_follow: None,
            active: None,
            draft: true,
            workspace_configs: Default::default(),
            ui_model_value: None,
            ui_mode: None,
            ui_plan: false,
            ui_thought: None,
            draft_models: Default::default(),
            composer: cx.new(crate::composer::input::Composer::new),
            session_drafts: Default::default(),
            draft_submission_overrides: Default::default(),
            recovered_attachments: Default::default(),
            composer_intent: Default::default(),
            client_id: "test-client".into(),
            log: Default::default(),
            errors: Default::default(),
            flow_saturated: Default::default(),
            assembler: crate::backend::wire::FrameAssembler::new(),
            route_cursors: Default::default(),
            child_owner: Default::default(),
            viewing_child: None,
            launch_candidates: vec![],
            retired_temp_files: vec![],
            profiles: Default::default(),
            phone: crate::remote::gateway::PhoneGateway::new(None),
            phone_client: Default::default(),
            browser: Default::default(),
            voice: None,
            system_prompt: Default::default(),
            secrets: Default::default(),
            provider_settings: Default::default(),
            project_activity: Default::default(),
            sidebar_cache: Default::default(),
        }
    }
}
