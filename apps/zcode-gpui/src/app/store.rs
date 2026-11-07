//! App state: one agent process per workspace (like the desktop's host), V4
//! subscriptions per workspace, and mirrored conversation state. UI-side only
//! — the backend owns sessions, admission and stream semantics; the frontend
//! mirrors snapshots/deltas and issues commands.
//!
//! Connection event handling lives in backend/events.rs, outbound actions in
//! backend/commands.rs / composer/config_cmds.rs.

use crate::backend::workspace::{WorkspaceHandle, WorkspacePurpose};
use crate::conversation::model::ConversationState;
use gpui::{AppContext, Context, Entity};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

/// The route cursor of the last frame applied on a topic: subscription
/// generation, host log epoch and the applied seq. Continuity is judged
/// against this exact triple — never against a topic-only seq (2026-10-05
/// audit P0.1; canonical rule: controller.ts `isWindowHostControllerFrameGap`).
#[derive(Clone, Debug)]
pub(crate) struct RouteCursor {
    pub(crate) subscription_id: String,
    pub(crate) log_epoch: String,
    pub(crate) seq: u64,
}

pub struct AppState {
    pub workspaces: Vec<WorkspaceHandle>,
    /// Workspace key of the context used for "New chat" (last selected).
    pub active_workspace: Option<String>,
    pub(crate) navigation_generation: u64,
    /// Bumped per plugin example prefill; the view then closes Settings to show the draft.
    pub(crate) plugin_prompt_applied: u64,
    pub(crate) waking_send: Option<crate::composer::waking_send::WakingSend>,
    pub(crate) held_confirmation:
        Option<std::sync::Arc<crate::composer::held_queue::HeldConfirmation>>,
    pub(crate) workflow_navigation_generation: u64,
    pub(crate) workflow_focus: Option<crate::backend::workflow_navigation::WorkflowFocus>,
    pub(crate) workflow_follow: Option<crate::backend::workflow_navigation::WorkflowFollow>,
    pub conversations: HashMap<String, ConversationState>,
    pub active: Option<String>,
    pub draft: bool,
    pub workspace_configs: HashMap<String, crate::composer::catalog::WorkspaceConfig>,
    /// Draft-mode selections, sent via createSession.config (CAS commands need a session).
    pub ui_model_value: Option<String>,
    pub ui_mode: Option<String>,
    /// Draft plan overlay (Desktop `planEnabled`), independent of `ui_mode`.
    pub ui_plan: bool,
    pub ui_thought: Option<String>,
    pub(crate) draft_models: crate::composer::draft_model::DraftModels,
    pub composer: Entity<crate::composer::input::Composer>,
    /// Per-session draft text saved when navigating between conversations.
    pub session_drafts: HashMap<String, String>,
    pub(crate) draft_submission_overrides: HashMap<String, Value>,
    pub(crate) recovered_attachments:
        HashMap<String, Vec<crate::composer::attachment::AttachmentRef>>,
    /// What Enter in the composer does (send / edit message / rename).
    pub composer_intent: crate::conversation::msg_actions::ComposerIntent,
    /// Stable per-install client identity for command envelopes (persisted
    /// once under the data dir; shared::identity owns the file lifecycle).
    pub(crate) client_id: String,
    pub(crate) log: VecDeque<String>,
    /// Dismissable error banners (most recent last, newest shown first).
    pub(crate) errors: VecDeque<String>,
    /// Per-workspace flow latch (true = we told the CLI to pause).
    pub(crate) flow_saturated: HashMap<String, bool>,
    pub(crate) assembler: crate::backend::wire::FrameAssembler,
    /// Last applied route cursor per topic (`(fromSeq, toSeq]` continuity +
    /// subscription generation). Cleared together with fragment state on
    /// reconnect/unsubscribe.
    pub(crate) route_cursors: HashMap<String, RouteCursor>,
    /// Child subagent session -> (workspace_key, parent_session_id)
    pub child_owner: HashMap<String, (String, String)>,
    /// Active child subagent session being viewed (read-only)
    pub viewing_child: Option<String>,
    /// Backend launch candidates for runtime-added workspaces (startup ones get their own copies).
    pub(crate) launch_candidates: Vec<crate::backend::launcher::BackendLaunch>,
    /// Pasted-image temp files: the backend may still read them, so they are deleted on quit.
    pub(crate) retired_temp_files: Vec<PathBuf>,
    pub(crate) profiles: crate::app::profile_state::ProfileState,
    /// Desktop: the phone gateway (listener, pairing, phone sockets) and the browser bridge.
    pub(crate) phone: crate::remote::gateway::PhoneGateway,
    pub(crate) browser: crate::browser_bridge::BrowserBridge,
    /// Phone build: the paired desktop that serves every workspace.
    pub(crate) phone_client: crate::remote::phone_state::PhoneClient,
    /// Desktop only (`None` on the phone): spoken output, see VOICE.md.
    pub(crate) voice: Option<crate::voice::VoiceService>,
    /// GPUI-only main-agent system prompt override (system_prompt/mod.rs).
    pub(crate) system_prompt: crate::system_prompt::SystemPromptState,
    /// GPUI-only secrets vault (SECRETS.md), pushed with `secrets/sync`.
    pub(crate) secrets: crate::secrets::SecretVault,
    pub(crate) provider_settings: crate::provider_settings::ProviderSettingsState,
    /// Remembered project activity for sidebar order (app/project_order.rs).
    pub(crate) project_activity: crate::shared::project_activity::ProjectActivity,
    /// Last known rows and open folders (app/sidebar_warm.rs).
    pub(crate) sidebar_cache: crate::shared::sidebar_cache::SidebarCache,
}

impl AppState {
    pub fn new(
        workspace_paths: Vec<PathBuf>,
        candidates: Vec<crate::backend::launcher::BackendLaunch>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::build(workspace_paths, candidates, false, cx)
    }

    /// Phone build: no local agents; workspaces come from the paired desktop.
    pub fn new_remote(cx: &mut Context<Self>) -> Self {
        Self::build(Vec::new(), Vec::new(), true, cx)
    }

    fn build(
        workspace_paths: Vec<PathBuf>,
        candidates: Vec<crate::backend::launcher::BackendLaunch>,
        remote: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let isolated = crate::shared::isolation::active().is_some();
        let mut state = Self {
            phone: crate::remote::gateway::PhoneGateway::new(
                (!remote && !isolated).then(crate::remote::pairing::PairingStore::path),
            ),
            phone_client: Default::default(),
            browser: Default::default(),
            voice: None,
            provider_settings: Default::default(),
            system_prompt: crate::system_prompt::SystemPromptState::for_app(remote),
            secrets: crate::secrets::SecretVault::for_app(remote),
            project_activity: Self::load_project_activity(remote),
            sidebar_cache: Self::load_sidebar_cache(remote),
            workspaces: workspace_paths
                .into_iter()
                .map(|p| WorkspaceHandle::new(p, candidates.clone()))
                .collect(),
            active_workspace: None,
            navigation_generation: 0,
            plugin_prompt_applied: 0,
            held_confirmation: None,
            waking_send: None,
            conversations: HashMap::new(),
            workflow_navigation_generation: 0,
            workflow_focus: None,
            workflow_follow: None,
            active: None,
            child_owner: HashMap::new(),
            viewing_child: None,
            launch_candidates: candidates.clone(),
            retired_temp_files: Vec::new(),
            profiles: {
                use crate::shared::profile_consent;
                let isolated = crate::shared::isolation::active().cloned();
                let consent_path = isolated.is_none().then(profile_consent::path);
                crate::app::profile_state::ProfileState {
                    consented: consent_path.as_deref().is_some_and(profile_consent::load),
                    consent_path,
                    isolated,
                    ..Default::default()
                }
            },
            draft: false,
            workspace_configs: HashMap::new(),
            ui_model_value: None,
            ui_mode: None,
            ui_plan: false,
            ui_thought: None,
            draft_models: crate::composer::draft_model::DraftModels::load(cx),
            composer: cx.new(crate::composer::input::Composer::new),
            session_drafts: HashMap::new(),
            draft_submission_overrides: HashMap::new(),
            recovered_attachments: HashMap::new(),
            composer_intent: Default::default(),
            client_id: crate::shared::identity::install_client_id(),
            log: VecDeque::new(),
            errors: VecDeque::new(),
            flow_saturated: HashMap::new(),
            assembler: crate::backend::wire::FrameAssembler::new(),
            route_cursors: HashMap::new(),
        };
        crate::composer::recall::wire_active_conversation(&state.composer, cx);
        if remote {
            state.start_remote_client(cx);
        } else {
            state.register_conversation_workspace(candidates);
            let primary = state
                .workspaces
                .iter()
                .find(|w| w.purpose == crate::backend::workspace::WorkspacePurpose::Project)
                .map(|w| w.key.clone());
            state.active_workspace =
                primary.or_else(|| state.workspaces.first().map(|w| w.key.clone()));
            if let Some(key) = state.active_workspace.clone() {
                state.spawn_workspace(&key, cx);
            }
            state.warm_sidebar(cx);
            state.init_phone_gateway(cx);
            state.init_browser_bridge(cx);
            state.load_secrets(cx);
            state.voice = Some(crate::voice::VoiceService::new());
        }
        state.start_maintenance(cx);
        crate::backend::flow_cmds::start_tail_poll(cx);
        // Startup sweep: paste temp files left behind by a crashed session.
        crate::shared::temp_attachments::scavenge_stale();
        // gpui does not guarantee entity drops at process exit, so `Drop`
        // alone can leave agents running where no kill-on-close job object
        // exists (non-Windows, or a failed job assignment).
        cx.on_app_quit(|this, _cx| {
            this.shutdown_all_workspaces();
            this.flush_project_activity_now();
            // Submitted pasted images may still be referenced by the backend
            // until now; quit is the point where they can finally go.
            for path in this.retired_temp_files.drain(..) {
                crate::shared::temp_attachments::delete_owned(&path);
            }
            async {}
        })
        .detach();
        state
    }

    pub(crate) fn push_error(&mut self, line: String) {
        // Error banners echo backend/provider text, which can quote keys.
        self.errors.push_back(crate::shared::redact::scrub(&line));
        while self.errors.len() > 3 {
            self.errors.pop_front();
        }
    }

    pub fn dismiss_error(&mut self, index: usize) {
        // UI renders newest-first; map back to deque order.
        let len = self.errors.len();
        if index < len {
            self.errors.remove(len - 1 - index);
        }
    }

    /// Terminate all workspace agents cleanly.
    pub fn shutdown_all_workspaces(&mut self) {
        self.profiles.shutdown();
        self.browser.shutdown(std::time::Instant::now());
        if let Some(voice) = &mut self.voice {
            voice.shutdown();
        }
        for w in &mut self.workspaces {
            w.shutdown();
        }
    }

    /// Open a picked folder as a project workspace (Electron parity:
    /// WorkspaceSidebar "+" → Open folder). Adds (or re-activates) the
    /// workspace, spawns its agent, and best-effort persists it to
    /// `setting.json` `recentProjects` — refused while the desktop runs (it
    /// rewrites the file whole), in which case the project still exists for
    /// this session.
    pub(crate) fn add_workspace(&mut self, raw: &Path, cx: &mut Context<Self>) {
        if let Some(isolated) = crate::shared::isolation::active()
            && !isolated.allows_workspace(raw)
        {
            self.push_error("Isolated Settings mode only permits disposable workspaces".into());
            cx.notify();
            return;
        }
        let canonical = crate::backend::workspace::canonical_workspace_string(raw);
        if !Path::new(&canonical).is_dir() {
            self.push_error(format!("not a directory: {canonical}"));
            return;
        }
        let key = match self.workspaces.iter().find(|w| w.key == canonical) {
            Some(w) => w.key.clone(),
            None => {
                let mut h =
                    WorkspaceHandle::new(PathBuf::from(&canonical), self.launch_candidates.clone());
                h.purpose = WorkspacePurpose::Project;
                let key = h.key.clone();
                self.workspaces.push(h);
                key
            }
        };
        self.active_workspace = Some(key.clone());
        self.spawn_workspace(&key, cx);
        crate::shared::preferences::Preferences::enqueue(
            crate::shared::preferences::PreferenceChange::RecentProject(canonical.clone()),
            cx,
        );
        self.push_log(format!("added project {canonical}"));
        cx.notify();
    }

    /// Spawn the agent for a workspace if it isn't running (lazy start).
    pub(crate) fn spawn_workspace(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(ws) = self.ws_mut(key) else {
            return;
        };
        if ws.pumping {
            return;
        }
        ws.pumping = true;
        ws.last_activity = Some(std::time::Instant::now());
        let Some(events) = ws.try_spawn() else {
            ws.pumping = false;
            ws.status = "no backend".into();
            let display = ws.display.clone();
            self.push_log(format!("no usable backend for workspace {key}"));
            self.push_error(format!(
                "no usable backend for {display} — install bun (or ship zcode-backend beside GPUI)"
            ));
            return;
        };
        if let (Some(backlog), Some(log_backlog)) = (ws.backlog.clone(), ws.log_backlog.clone()) {
            crate::backend::events::attach_pump(
                cx,
                key.to_string(),
                ws.generation,
                backlog,
                log_backlog,
                events,
            );
        }
        cx.notify();
    }

    pub(crate) fn ws(&self, key: &str) -> Option<&WorkspaceHandle> {
        self.workspaces.iter().find(|w| w.key == key)
    }

    pub(crate) fn ws_mut(&mut self, key: &str) -> Option<&mut WorkspaceHandle> {
        self.workspaces.iter_mut().find(|w| w.key == key)
    }

    /// Filesystem path of the active workspace (git + terminal + file tree).
    pub(crate) fn active_workspace_path(&self) -> Option<std::path::PathBuf> {
        let key = self.active_ws_key()?;
        self.ws(&key).map(|w| w.path.clone())
    }

    /// Workspace of the active session if known, else the last active one.
    pub(crate) fn active_ws_key(&self) -> Option<String> {
        self.active
            .as_ref()
            .and_then(|sid| {
                self.workspaces
                    .iter()
                    .find(|w| w.sessions.iter().any(|s| &s.session_id == sid))
                    .map(|w| w.key.clone())
            })
            .or_else(|| self.active_workspace.clone())
    }

    pub(crate) fn active_sessions(&self) -> Vec<(String, String)> {
        let key = self.active_ws_key();
        self.workspaces
            .iter()
            .find(|w| key.as_ref() == Some(&w.key))
            .map(|w| {
                w.sessions
                    .iter()
                    .map(|s| {
                        let t = s.title.trim();
                        let title = if t.is_empty() { &s.session_id } else { t };
                        (s.session_id.clone(), title.to_string())
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn active_conversation(&self) -> Option<&ConversationState> {
        let sid = crate::conversation::subagent_nav::active_conversation_sid(
            self.viewing_child.as_deref(),
            self.active.as_deref(),
        )?;
        self.conversations.get(sid)
    }

    pub(crate) fn push_log(&mut self, line: String) {
        let line = crate::shared::redact::scrub(&line);
        if std::env::var("ZCODE_GPUI_LOG_STDOUT").is_ok() {
            eprintln!("[zcode-gpui] {line}");
        }
        self.log.push_back(line);
        while self.log.len() > 200 {
            self.log.pop_front();
        }
    }

    pub(crate) fn status_error(&mut self, ws_key: &str, msg: &str) {
        let msg = crate::backend::status::set_workspace_error(&mut self.workspaces, ws_key, msg);
        self.push_log(format!("status: {msg}"));
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        self.shutdown_all_workspaces();
        for path in &self.retired_temp_files {
            crate::shared::temp_attachments::delete_owned(path);
        }
    }
}
