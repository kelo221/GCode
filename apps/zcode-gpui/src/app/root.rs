//! Root layout: sidebar (all projects + sessions), header selectors, chat
//! transcript, mode bar and composer.

use crate::app::dock::{DockTab, ToggleDock};
use crate::app::store::AppState;
use crate::composer::input::Composer;
use crate::conversation::model::format_preview;
use crate::files::pane::FilesState;
use crate::review::git::GitState;
use crate::shared::theme::{FRAME, TEXT};
use crate::shared::theme_colors::color as rgb;
use crate::terminal::pane::{TermPane, ToggleTerminal};
use gpui::{
    Context, Entity, IntoElement, ListState, MouseButton, ParentElement, Render, SharedString,
    Styled, Window, div, prelude::*,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub struct RootView {
    pub(crate) state: Entity<AppState>,
    /// Virtualized transcript state (see transcript/list.rs).
    pub(crate) list_state: ListState,
    pub(crate) row_index: Vec<u64>,
    pub(crate) last_bounds: (u64, u64),
    /// Session-scoped row IDs require a list reset when switching conversations.
    pub(crate) last_session: Option<String>,
    pub(crate) follow_bottom: Arc<AtomicBool>,
    /// Transcript area focus: clicking it releases the composer so Home/End scroll.
    pub(crate) transcript_focus: gpui::FocusHandle,
    pub(crate) last_row_count: usize,
    pub(crate) last_first_row_id: u64,
    /// Thinking rows flipped against the default (open in noise-free mode, closed otherwise).
    pub(crate) toggled_reasonings: HashSet<u64>,
    pub(crate) expanded_tools: HashSet<u64>,
    /// Expanded noise-free fold runs, keyed by the run's first row id.
    pub(crate) expanded_folds: HashSet<u64>,
    pub(crate) plan_expanded: bool,
    pub(crate) agents_expanded: bool,
    /// Destructive action awaiting a second click ("del:<sid>", "undo:<rowId>").
    pub(crate) confirm: Option<String>,
    /// Question picks per interaction: interactionId → question index → option values.
    pub(crate) question_picks: HashMap<String, HashMap<usize, Vec<String>>>,
    /// Question stepper position per interaction (interactionId → question index).
    pub(crate) question_steps: HashMap<String, usize>,
    pub(crate) interaction_input: crate::conversation::interaction_input::InteractionInput,
    pub(crate) tool_images: crate::transcript::tool_images::ToolImageCache,
    pub(crate) dock_open: bool,
    pub(crate) dock_tab: DockTab,
    pub(crate) term_open: bool,
    pub(crate) git: GitState,
    pub(crate) commit_input: Entity<Composer>,
    pub(crate) files: FilesState,
    pub(crate) term: TermPane,
    /// Phase of the last render; a completed turn refreshes git status.
    last_phase: String,
    auto_opened_this_turn: bool,
    last_plan_present: bool,
    seen_plugin_prompt: u64,
    pub(crate) middle_scroll: Option<crate::transcript::scroll::MiddleScrollState>,
    /// An autoscroll `on_next_frame` callback is queued (see transcript/scroll.rs).
    pub(crate) autoscroll_frame_pending: bool,
    pub(crate) quickpick_open: bool,
    pub(crate) quickpick_query: String,
    pub(crate) quickpick_selected: usize,
    pub(crate) os_lifecycle: crate::app::os_lifecycle::OsLifecycleState,
    pub(crate) plugin_segment: crate::app::plugin_pane::PluginSegment,
    pub(crate) folders: crate::sessions::folders::Folders,
    /// Per-workspace session-row step (0=3 latest, 1=extended, 2=all).
    pub(crate) session_limit_step: HashMap<String, u8>,
    pub(crate) settings: crate::app::settings::SettingsView,
    pub(crate) subagents: crate::app::subagents_settings::SubagentsView,
    pub(crate) composer_compact: bool,
    /// Suggestion-popup highlight: the composer text it was picked for, and the row.
    pub(crate) autocomplete_pick: (String, usize),
    /// Conversation column width from the last layout pass (transcript rows).
    pub(crate) conversation_column: f32,
    /// Phone layout page (REMOTE_CONTROL.md "Phone layout"); ignored when wide.
    pub(crate) mobile_page: crate::app::mobile_shell::MobilePage,
}

impl RootView {
    /// False on the phone build: the dock (git review, files, workflows,
    /// plugins) and the terminal act on this device, not the desktop.
    pub(crate) fn desktop_tools(&self, cx: &gpui::App) -> bool {
        !self.state.read(cx).phone_client.enabled
    }

    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        cx.observe_global::<ely_gpui_component::theme::Theme>(|_, cx| cx.notify())
            .detach();
        let preferences = cx
            .global::<crate::shared::preferences::PreferenceOwner>()
            .0
            .clone();
        cx.observe(&preferences, |_, _, cx| cx.notify()).detach();
        let composer = state.read(cx).composer.clone();
        let (list_state, follow_bottom) = crate::transcript::list::new_list_state();
        let commit_input = cx.new(|cx| Composer::new_single_line("Commit message", cx));
        Self::wire_submit_events(&composer, &commit_input, cx);
        Self {
            state,
            composer_compact: true,
            autocomplete_pick: Default::default(),
            conversation_column: 672.,
            mobile_page: Default::default(),
            list_state,
            row_index: Vec::new(),
            last_bounds: (0, 0),
            last_session: None,
            follow_bottom,
            transcript_focus: cx.focus_handle(),
            last_row_count: 0,
            last_first_row_id: 0,
            toggled_reasonings: HashSet::new(),
            expanded_tools: HashSet::new(),
            expanded_folds: HashSet::new(),
            plan_expanded: false,
            agents_expanded: true,
            confirm: None,
            question_picks: HashMap::new(),
            question_steps: HashMap::new(),
            interaction_input: crate::conversation::interaction_input::InteractionInput::new(cx),
            tool_images: Default::default(),
            commit_input,
            dock_open: false,
            dock_tab: DockTab::Review,
            term_open: false,
            git: GitState::default(),
            files: FilesState::default(),
            term: TermPane::new(cx.focus_handle()),
            last_phase: String::new(),
            auto_opened_this_turn: false,
            last_plan_present: false,
            seen_plugin_prompt: 0,
            middle_scroll: None,
            autoscroll_frame_pending: false,
            quickpick_open: false,
            quickpick_query: String::new(),
            quickpick_selected: 0,
            os_lifecycle: crate::app::os_lifecycle::OsLifecycleState::default(),
            plugin_segment: crate::app::plugin_pane::PluginSegment::Public,
            folders: Default::default(),
            session_limit_step: HashMap::new(),
            subagents: Default::default(),
            settings: crate::app::settings::SettingsView::new(cx.focus_handle()),
        }
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.restore_settings_focus(window, cx);
        self.guard_input_ownership(window, cx);
        let state = self.state.read(cx);
        let is_read_only = state.is_read_only_view();
        let active_sid = state.active.clone();
        let is_draft = state.draft;
        let row_count = state.active_conversation().map_or(0, |c| c.rows.len());
        let running = state
            .active_conversation()
            .map(|c| c.phase_running())
            .unwrap_or(false);
        let phase = state
            .active_conversation()
            .map(|c| c.phase.clone())
            .unwrap_or_default();
        let title: SharedString = match (&active_sid, is_draft, is_read_only) {
            (_, _, true) => "Subagent (read-only)".into(),
            (None, _, _) => "New task".into(),
            (Some(sid), _, _) => {
                let title = state
                    .workspaces
                    .iter()
                    .find_map(|w| {
                        w.sessions
                            .iter()
                            .find(|s| &s.session_id == sid)
                            .map(|s| s.title.clone())
                    })
                    .unwrap_or_else(|| "Session".into());
                format_preview(&title, 60).into()
            }
        };

        let context_tag: Option<SharedString> = if is_read_only {
            Some("Subagent".into())
        } else if let Some(sid) = &active_sid {
            state.workspaces.iter().find_map(|w| {
                w.sessions
                    .iter()
                    .any(|s| &s.session_id == sid)
                    .then(|| match w.purpose {
                        crate::backend::workspace::WorkspacePurpose::Conversation => "Tasks".into(),
                        crate::backend::workspace::WorkspacePurpose::Project => {
                            w.display.clone().into()
                        }
                    })
            })
        } else if let Some(ws_key) = state.active_ws_key() {
            if state.is_conversation_workspace(&ws_key) {
                Some("Tasks".into())
            } else {
                state.ws(&ws_key).map(|w| w.display.clone().into())
            }
        } else {
            None
        };

        if !cfg!(test) {
            crate::app::os_lifecycle::sync_window_state(
                window,
                &mut self.os_lifecycle.last_saved_bounds,
            );
            crate::app::os_lifecycle::sync_keep_awake(&mut self.os_lifecycle.keep_awake, running);
            crate::app::os_lifecycle::check_turn_completion_notification(
                &self.last_phase,
                &phase,
                &title,
                window.is_window_active(),
            );
        }
        let composer = state.composer.clone();
        let first_row_id = state
            .active_conversation()
            .and_then(|c| c.loaded_first_row_id())
            .unwrap_or(0);
        let has_more = state
            .active_conversation()
            .map(|c| c.has_more_history())
            .unwrap_or(false);
        let phase_error = state
            .active_conversation()
            .filter(|c| c.phase == "error")
            .and_then(|c| c.last_error.clone());
        let errors: Vec<String> = state.errors.iter().cloned().collect();
        let plan = state.active_conversation().and_then(|c| c.plan.clone());
        let pending_interactions = state
            .active_conversation()
            .map(|c| c.pending_interactions.clone())
            .unwrap_or_default();
        let queue = state.active_conversation().and_then(|c| c.queue.clone());
        let intent = state.composer_intent.clone();
        // Reconcile the virtual list with the model before building elements.
        self.sync_list(row_count, first_row_id, cx);
        let rows_empty = row_count == 0;
        // Auto-refresh the Review pane when a turn finishes writing files.
        if self.last_phase != phase {
            if crate::conversation::model::phase_is_active(&phase)
                && !crate::conversation::model::phase_is_active(&self.last_phase)
            {
                self.auto_opened_this_turn = false;
            }
            if phase == "completedSuccess" {
                // Collapse the plan capsule once its work is done; the user
                // can re-expand it. Also refresh the Review pane if open.
                self.plan_expanded = false;
                if self.dock_open {
                    self.refresh_git(cx);
                }
            }
            self.last_phase = phase.clone();
        }
        // Desktop parity: a turn with a plan opens the side panel once, so the
        // progress capsule is visible without hunting for it. A manual close
        // is respected for the rest of the turn.
        let plan_present = plan.as_ref().is_some_and(|p| !p.items.is_empty());
        if plan_present
            && !self.last_plan_present
            && !self.auto_opened_this_turn
            && self.desktop_tools(cx)
        {
            self.dock_open = true;
            self.dock_tab = crate::app::dock::DockTab::Review;
            self.auto_opened_this_turn = true;
            self.on_dock_tab(crate::app::dock::DockTab::Review, cx);
        }
        self.last_plan_present = plan_present;
        let plugin_prompt_applied = self.state.read(cx).plugin_prompt_applied;
        if plugin_prompt_applied != self.seen_plugin_prompt {
            self.seen_plugin_prompt = plugin_prompt_applied;
            if self.settings.open {
                self.close_settings(window, cx);
            }
        }

        let compact = crate::app::mobile_shell::is_compact(f32::from(window.viewport_size().width));
        let chat_page = compact && self.mobile_page == crate::app::mobile_shell::MobilePage::Chat;
        div()
            .size_full()
            .flex()
            .flex_row()
            .font_family(crate::shared::theme::UI_FONT)
            .bg(rgb(FRAME))
            .text_color(rgb(TEXT))
            .key_context("Root")
            .on_action(cx.listener(|this, _: &ToggleDock, _, cx| {
                this.dock_open = !this.dock_open;
                if this.dock_open {
                    this.on_dock_tab(this.dock_tab, cx);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleTerminal, window, cx| {
                this.term_open = !this.term_open;
                if this.term_open {
                    this.ensure_term(cx);
                    window.focus(&this.term.focus, cx);
                }
                cx.notify();
            }))
            .on_action(cx.listener(
                |this, _: &crate::app::quickpick::ToggleQuickPick, window, cx| {
                    if this.quickpick_open {
                        this.close_command_center(window, cx);
                    } else {
                        this.open_command_center(window, cx);
                    }
                },
            ))
            .map(|el| self.navigation_handlers(el, cx))
            // Autoscroll gestures are window-wide: any other button cancels
            // (capture phase, so buttons that stop propagation still cancel),
            // and move/release keep tracking outside the transcript.
            .capture_any_mouse_down(cx.listener(|this, ev: &gpui::MouseDownEvent, _, cx| {
                if ev.button != MouseButton::Middle {
                    this.cancel_middle_scroll(cx);
                }
            }))
            .on_mouse_move(cx.listener(|this, ev: &gpui::MouseMoveEvent, _, cx| {
                this.handle_middle_move(ev, cx);
            }))
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|this, ev: &gpui::MouseUpEvent, _, cx| {
                    this.handle_middle_up(ev, cx);
                }),
            )
            // Settings 是独立目的地；只切换渲染，不能卸载 workspace 或改变其业务状态。
            .when(
                !self.settings.open && !chat_page && !self.sidebar_hidden(compact),
                |el| el.child(self.sidebar(compact, cx)),
            )
            // Main column retains the workspace panel outside Settings; the
            // phone layout shows it only on the chat page.
            .children(if self.settings.open {
                Some(self.render_settings(window, cx))
            } else if compact && !chat_page {
                None
            } else {
                let panel = self.workspace_panel(
                    crate::app::workspace_panel::PanelInputs {
                        title,
                        context_tag,
                        plan,
                        phase,
                        errors,
                        phase_error,
                        has_more,
                        is_read_only,
                        is_draft,
                        pending_interactions,
                        rows_empty,
                        queue,
                        intent,
                        composer,
                        running,
                    },
                    window,
                    cx,
                );
                Some(if chat_page {
                    div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .child(self.mobile_chat_header(cx))
                        .child(panel)
                        .into_any_element()
                } else {
                    panel
                })
            })
            .children(
                self.quickpick_open
                    .then(|| crate::app::quickpick::render_quickpick_modal(self, window, cx)),
            )
            .children(self.render_image_viewer(window, cx))
    }
}
