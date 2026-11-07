//! Smaller pieces of the root layout: the composer intent
//! banner (editing a message / renaming a session) and the composer card.

use crate::app::root::RootView;
use crate::composer::input::Composer;
use crate::conversation::msg_actions::ComposerIntent;
use crate::shared::theme::ui_size;
use crate::shared::theme::{
    ACCENT, AMBER, BORDER, BORDER_HOVER, CARD, I_ARROW_UP, INPUT, MUTED, PANEL, TEXT,
};
use crate::shared::theme_colors::color as rgb;
use gpui::{
    AnyElement, ClickEvent, Context, CursorStyle, Div, Entity, ParentElement, Stateful, Styled,
    div, prelude::*, px,
};

impl RootView {
    pub(crate) fn queue_panel(
        &self,
        queue: Option<&crate::conversation::queue::QueueState>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        queue.and_then(|queue| {
            crate::conversation::queue::render_queue_panel(queue, &self.state, cx)
        })
    }

    /// "Editing message" / "Renaming session" strip above the composer.
    pub(crate) fn intent_banner(
        &self,
        intent: &ComposerIntent,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let label = match intent {
            ComposerIntent::Send => return None,
            ComposerIntent::Edit { .. } => {
                "Editing message: Enter resends it and rewinds the conversation to this point"
            }
            ComposerIntent::Rename { .. } => "Renaming session: Enter saves the new title",
        };
        Some(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(rgb(CARD))
                .border_1()
                .border_color(rgb(AMBER))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(ui_size(11.5)))
                        .text_color(rgb(AMBER))
                        .child(label),
                )
                .child(
                    div()
                        .id("intent-cancel")
                        .px_1p5()
                        .text_size(px(ui_size(11.5)))
                        .text_color(rgb(MUTED))
                        .hover(|s| s.text_color(rgb(TEXT)))
                        .cursor(CursorStyle::PointingHand)
                        .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                            cx.stop_propagation();
                            this.state.update(cx, |s, cx| s.cancel_intent(cx));
                        }))
                        .child("Cancel"),
                )
                .into_any_element(),
        )
    }

    pub(crate) fn load_earlier_btn(
        &self,
        has_more: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !has_more {
            return None;
        }
        let button = div()
            .id("load-earlier")
            .mx_auto()
            .px_3()
            .py_1()
            .rounded_md()
            .bg(rgb(PANEL))
            .border_1()
            .border_color(rgb(BORDER))
            .text_size(px(ui_size(11.5)))
            .text_color(rgb(MUTED))
            .cursor(CursorStyle::PointingHand)
            .hover(|s| s.bg(rgb(CARD)).text_color(rgb(TEXT)))
            .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                this.state.update(cx, |s, cx| s.fetch_earlier_rows(cx));
            }))
            .child("Load earlier messages");
        let wrapper = div().flex().child(button);
        #[cfg(test)]
        let wrapper =
            crate::app::test_support::track_children(wrapper, vec!["load-earlier".into()]);
        Some(wrapper.into_any_element())
    }
}

fn round_button(
    id: &'static str,
    glyph: crate::shared::theme::IconName,
    bg: u32,
    fg: gpui::Rgba,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .bg(rgb(bg))
        .cursor(CursorStyle::PointingHand)
        .child(gpui::svg().path(glyph.path()).size(px(16.)).text_color(fg))
}

impl RootView {
    /// Desktop-style composer: one rounded card with the input on top and
    /// mode / model / thinking pickers plus the round send button below.
    pub(crate) fn composer_card(
        &mut self,
        composer: Entity<Composer>,
        running: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let send = round_button(
            "send-btn",
            I_ARROW_UP,
            ACCENT,
            crate::shared::theme::active_theme().inverse,
        )
        .hover(|s| s.opacity(0.85))
        .on_click(cx.listener(|this, event: &ClickEvent, _window, cx| {
            let trigger = if crate::composer::delivery::primary_modifier(&event.modifiers()) {
                crate::composer::delivery::SubmitTrigger::ModifiedPointer
            } else {
                crate::composer::delivery::SubmitTrigger::Ordinary
            };
            this.state
                .update(cx, |s, cx| s.submit_composer_with_trigger(trigger, cx));
        }));
        let stop = crate::app::stop_button::working_stop_button().on_click(cx.listener(
            |this, _: &ClickEvent, _window, cx| {
                this.state.update(cx, |s, cx| s.stop(cx));
            },
        ));
        // Electron parity: while the agent works an empty draft offers only
        // Stop; with a draft, Send (which queues or steers) sits beside it.
        let has_draft = {
            let c = composer.read(cx);
            !c.text().trim().is_empty() || !c.attachments().is_empty()
        };
        let show_send = !running || has_draft;

        let query = crate::composer::references::reference_query(composer.read(cx).text())
            .map(|(kind, _, _)| kind);
        if let Some(kind) = query {
            self.state.update(cx, |state, cx| {
                state.ensure_reference_catalog(kind, false, cx)
            });
        }
        if crate::composer::slash::filter_slash_commands(&[], composer.read(cx).text()).is_some() {
            self.state
                .update(cx, |state, cx| state.ensure_slash_catalog(false, cx));
        }
        let suggestions = self.autocomplete_suggestions(cx);
        let indexing = suggestions.is_some()
            && self
                .active_workspace_path(cx)
                .is_some_and(|root| crate::composer::file_index::pending(&root));
        if indexing {
            // The index builds or ranks on its own thread; look again shortly for its results.
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        let popup = suggestions.map(|s| self.render_autocomplete_popup(&s, cx));
        let att_tray = Self::render_attachment_tray(&composer, cx);
        let attach_btn = Self::render_attach_button(cx);
        let view = cx.weak_entity();
        let held_confirmation = self.held_queue_confirmation(cx);
        let new_task = self
            .state
            .read(cx)
            .active_conversation()
            .is_none_or(|c| c.rows.is_empty());
        if composer.read(cx).new_task != new_task {
            composer.update(cx, |c, _| c.new_task = new_task);
        }

        let container = self.autocomplete_keys(div(), cx);
        container
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .on_mouse_down_out(
                cx.listener(|this, _, window, cx| this.release_phone_composer_focus(window, cx)),
            )
            .children(held_confirmation)
            .children(self.image_upload_feedback(cx))
            .children(self.reference_feedback(cx))
            .children(self.slash_catalog_feedback(cx))
            .children(self.waking_send_feedback(cx))
            .children(popup)
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_3()
                    .rounded_2xl()
                    .bg(rgb(INPUT))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .hover(|s| s.border_color(rgb(BORDER_HOVER)))
                    .on_children_prepainted(move |bounds, _, cx| {
                        if let Some(bounds) = bounds.last() {
                            let compact = bounds.size.width <= px(480.);
                            let _ = view.update(cx, |this, cx| {
                                if this.composer_compact != compact {
                                    this.composer_compact = compact;
                                    cx.notify();
                                }
                            });
                        }
                    })
                    .children(att_tray)
                    .child(composer)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .w_full()
                            .min_w_0()
                            .flex_wrap()
                            .items_center()
                            .gap_1()
                            .child(attach_btn)
                            .child(self.mode_menu(cx))
                            .child(self.noise_toggle(cx))
                            .children(self.auto_compact_toggle(cx))
                            .children(self.composer_activity(cx))
                            .child(self.composer_trailing(running, show_send, stop, send, cx)),
                    ),
            )
    }

    /// Model pickers plus Stop/Send. One group, pushed right by `ml_auto`, so
    /// when the row wraps on a narrow composer the buttons wrap with the
    /// pickers and stay right-aligned instead of starting a lone line.
    fn composer_trailing(
        &mut self,
        running: bool,
        show_send: bool,
        stop: impl IntoElement,
        send: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .ml_auto()
            .flex()
            .flex_row()
            .flex_wrap()
            .justify_end()
            .items_center()
            .gap_1()
            .min_w_0()
            .child(self.composer_selectors(cx))
            .when(running, |el| {
                el.child({
                    let wrapper = div().child(stop);
                    #[cfg(test)]
                    let wrapper =
                        crate::app::test_support::track_children(wrapper, vec!["stop-btn".into()]);
                    wrapper
                })
            })
            .when(show_send, |el| {
                el.child({
                    let wrapper = div().child(send);
                    #[cfg(test)]
                    let wrapper =
                        crate::app::test_support::track_children(wrapper, vec!["send-btn".into()]);
                    wrapper
                })
            })
    }

    pub(crate) fn autocomplete_suggestions(
        &self,
        cx: &Context<Self>,
    ) -> Option<Vec<crate::composer::autocomplete::AutocompleteSuggestion>> {
        let state = self.state.read(cx);
        let text = state.composer.read(cx).text().to_string();
        let slash = state.active_slash_commands().unwrap_or_default();
        let sessions = state.active_sessions();
        let ws_path = state.active_workspace_path();
        let mut references = self.reference_suggestions(&text, cx);
        references.extend(
            crate::composer::autocomplete::detect_autocomplete(
                &text,
                slash,
                &sessions,
                ws_path.as_deref(),
            )
            .unwrap_or_default(),
        );
        references.truncate(8);
        (!references.is_empty()).then_some(references)
    }

    /// Filesystem path of the active workspace (git, terminal, file tree).
    pub(crate) fn active_workspace_path(&self, cx: &Context<Self>) -> Option<std::path::PathBuf> {
        self.state.read(cx).active_workspace_path()
    }

    pub(crate) fn submit(&mut self, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| state.submit_composer(cx));
    }
}
