//! Workspace destination outside Settings: the centre panel (header, transcript,
//! composer, terminal) and, when open and it fits, the docked side pane.

use crate::app::layout::{
    CENTER_MIN_WIDTH, PANEL_GAP, PANEL_RADIUS, SIDE_PANE_WIDTH, SIDEBAR_WIDTH,
    conversation_padding, conversation_width, side_pane_fits,
};
use crate::app::mobile_shell::is_compact;
use crate::app::root::RootView;
use crate::composer::input::Composer;
use crate::conversation::turn_meta::PlanState;
use crate::shared::theme::{BORDER, PANEL};
use crate::shared::theme_colors::color as rgb;
use gpui::{AnyElement, Context, Entity, SharedString, Window, div, prelude::*, px};

pub(crate) struct PanelInputs {
    pub title: SharedString,
    pub context_tag: Option<SharedString>,
    pub plan: Option<PlanState>,
    pub phase: String,
    pub errors: Vec<String>,
    pub phase_error: Option<(String, String)>,
    pub has_more: bool,
    pub is_read_only: bool,
    pub is_draft: bool,
    pub pending_interactions: Vec<crate::conversation::interactions::PendingInteraction>,
    pub rows_empty: bool,
    pub queue: Option<crate::conversation::queue::QueueState>,
    pub intent: crate::conversation::msg_actions::ComposerIntent,
    pub composer: Entity<Composer>,
    pub running: bool,
}

impl RootView {
    pub(crate) fn workspace_panel(
        &mut self,
        input: PanelInputs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let window_width = f32::from(window.viewport_size().width);
        // The dock and terminal work on this machine's files; the phone build
        // only mirrors the desktop's sessions (REMOTE_CONTROL.md, phone scope).
        let tools = self.desktop_tools(cx);
        let dock_open = self.dock_open && tools;
        let compact = is_compact(window_width);
        let sidebar_visible = !self.sidebar_hidden(compact);
        let docked = dock_open && side_pane_fits(window_width, sidebar_visible);
        let side = if docked {
            SIDE_PANE_WIDTH + PANEL_GAP
        } else {
            0.
        };
        let sidebar = if sidebar_visible { SIDEBAR_WIDTH } else { 0. };
        let center = if compact {
            window_width - 2. * PANEL_GAP
        } else {
            (window_width - sidebar - PANEL_GAP - side).max(CENTER_MIN_WIDTH)
        };
        let column = conversation_width(center, input.is_draft || input.rows_empty);
        let padding = conversation_padding(center);
        self.conversation_column = column;
        let PanelInputs {
            title,
            context_tag,
            plan,
            phase,
            errors,
            phase_error,
            has_more,
            is_read_only,
            pending_interactions,
            rows_empty,
            queue,
            intent,
            composer,
            running,
            ..
        } = input;
        // Desktop hides the composer behind a blocking interaction, so typed
        // text can only answer it (ConversationComposer `display:none`).
        let blocking = if is_read_only {
            None
        } else {
            crate::conversation::interaction_input::blocking(&pending_interactions).cloned()
        };
        self.sync_interaction_input(blocking.as_ref(), window, cx);
        let shown: Vec<_> = blocking
            .iter()
            .cloned()
            .chain(
                pending_interactions
                    .iter()
                    .filter(|pi| {
                        !is_read_only && !crate::conversation::interaction_input::is_blocking(pi)
                    })
                    .cloned(),
            )
            .collect();
        let panel = |el: gpui::Div| {
            el.rounded(px(PANEL_RADIUS))
                .overflow_hidden()
                .bg(rgb(PANEL))
                .border_1()
                .border_color(rgb(BORDER))
        };
        let composer_region = div()
            .w(px(column))
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1p5()
            .pt_1()
            .pb(px(padding / 2.))
            .when(!is_read_only, |el| {
                el.children(self.queue_panel(queue.as_ref(), cx))
                    .children(self.intent_banner(&intent, cx))
                    .children(self.interaction_cards(&shown, cx))
                    .when(blocking.is_none(), |el| {
                        el.child(self.composer_card(composer, running, cx))
                    })
            })
            .when(is_read_only, |el| {
                el.child(self.subagent_read_only_banner(cx))
            });
        // Desktop draft: greeting and composer centred together; otherwise the
        // composer docks under the transcript.
        let (body, bottom) = if rows_empty {
            let draft = div()
                .w_full()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .child(crate::app::draft_greeting::draft_greeting(
                    column,
                    window.scale_factor(),
                    cx,
                ))
                .child(composer_region);
            (draft.into_any_element(), None)
        } else {
            let bottom = div()
                .w_full()
                .flex()
                .justify_center()
                .child(composer_region);
            (self.transcript_list(cx), Some(bottom))
        };
        // Files dropped anywhere on the chat panel attach to the composer.
        let centre = panel(div().flex_1().min_w_0().flex().flex_col())
            .drag_over::<gpui::ExternalPaths>(|style, _, _, _| {
                style.bg(crate::shared::theme_colors::tinted(
                    crate::shared::theme::ACCENT,
                    0.08,
                ))
            })
            .on_drop(
                cx.listener(|this, paths: &gpui::ExternalPaths, _window, cx| {
                    this.attach_files(paths.paths().to_vec(), cx);
                }),
            )
            .child(self.main_header(title, context_tag, plan.as_ref(), &phase, cx))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .items_center()
                    .flex_1()
                    .min_h_0()
                    .children(self.error_banners(errors, phase_error, cx))
                    // Pinned strip above the transcript: history paging and subagent
                    // navigation.
                    .child(
                        div()
                            .w(px(column))
                            .pt_2()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .children(self.subagent_back_bar(cx))
                            .children(self.load_earlier_btn(has_more, cx)),
                    )
                    .child(body)
                    // Narrow windows keep the side pane as an overlay.
                    .when(dock_open && !docked, |el| {
                        el.child(self.dock_pane(false, cx))
                    }),
            )
            .children(bottom)
            .when(self.term_open && tools, |el| {
                el.child(self.term_drawer(window, cx))
            });
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_row()
            .gap(px(PANEL_GAP))
            .py(px(PANEL_GAP))
            .pr(px(PANEL_GAP))
            .when(compact, |el| el.pl(px(PANEL_GAP)))
            .child(centre)
            .when(docked, |el| el.child(self.dock_pane(true, cx)))
            .into_any_element()
    }
}
