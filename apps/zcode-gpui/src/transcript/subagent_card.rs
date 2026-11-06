//! Transcript card rendering for subagents.
//!
//! Spec: Status dot, `<subagentType> · <status>`, clamped summary,
//! elapsed duration while running, Open child session button, and Stop button.

use crate::app::root::RootView;
use crate::conversation::model::Row;
use crate::shared::theme::ui_size;
use crate::shared::theme::{ACCENT, BORDER, CARD, CARD_HOVER, DANGER, MUTED, PANEL, SUCCESS, TEXT};
use crate::shared::theme_colors::color as rgb;
use gpui::{
    AnyElement, ClipboardItem, Context, ElementId, IntoElement, ParentElement, Styled, div,
    prelude::*, px,
};

/// The CLI names a subagent's child session `sess_subagent_<agentId>`, so the
/// agent ID (what SendMessage takes) is recoverable from any subagent row.
pub(crate) fn agent_id_from_child_session(child_session_id: &str) -> Option<&str> {
    child_session_id
        .strip_prefix("sess_subagent_")
        .filter(|id| id.starts_with("agent_"))
}

/// `agent_7fbbd55f-28a1-…` → `agent_7fbbd55f` for the chip label.
pub(crate) fn short_agent_id(agent_id: &str) -> &str {
    let end = agent_id
        .char_indices()
        .skip("agent_".len())
        .find(|(_, c)| *c == '-')
        .map_or(agent_id.len(), |(i, _)| i);
    &agent_id[..end]
}

impl RootView {
    /// Small mono chip that copies the full agent ID, so the user can tell the
    /// agent "resume agent_…" after a restart or lost connection.
    pub(crate) fn agent_id_copy_chip(&self, agent_id: &str, cx: &mut Context<Self>) -> AnyElement {
        let full = agent_id.to_string();
        let element = div().child(
            div()
                .id(ElementId::Name(format!("agent-id-copy-{agent_id}").into()))
                .px_1p5()
                .py_0p5()
                .rounded_sm()
                .bg(rgb(PANEL))
                .border_1()
                .border_color(rgb(BORDER))
                .font_family(crate::shared::theme::MONO_FONT)
                .text_size(px(ui_size(10.)))
                .text_color(rgb(MUTED))
                .hover(|h| h.bg(rgb(CARD_HOVER)).text_color(rgb(TEXT)))
                .cursor_pointer()
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.write_to_clipboard(ClipboardItem::new_string(full.clone()));
                }))
                .child(short_agent_id(agent_id).to_string()),
        );
        #[cfg(test)]
        let element = crate::app::test_support::track_children(
            element,
            vec![format!("agent-id-copy-{agent_id}")],
        );
        element.into_any_element()
    }

    pub(crate) fn find_subagent_for_tool_call(
        &self,
        tool_call_id: &str,
        cx: &Context<Self>,
    ) -> Option<Row> {
        let state = self.state.read(cx);
        state
            .active_conversation()?
            .find_subagent_for_tool_call(tool_call_id)
            .cloned()
    }

    pub(crate) fn has_tool_call_with_id(&self, tool_call_id: &str, cx: &Context<Self>) -> bool {
        let state = self.state.read(cx);
        state
            .active_conversation()
            .is_some_and(|c| c.has_tool_call_with_id(tool_call_id))
    }

    pub(crate) fn render_subagent_card(
        &self,
        row: &Row,
        is_nested: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Row::Subagent {
            row_id,
            subagent_type,
            status,
            summary_text,
            child_session_id,
            work_id,
            started_at,
            ..
        } = row
        else {
            return div().into_any_element();
        };

        let r_id = *row_id;
        let is_running = status == "running";
        let is_failed = status == "failed" || status == "error";
        let is_success = status == "success" || status == "completed";

        let dot_color = if is_failed {
            DANGER
        } else if is_running {
            ACCENT
        } else if is_success {
            SUCCESS
        } else {
            MUTED
        };

        let elapsed = if is_running {
            started_at.map(|t| {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(t);
                let diff = now_ms.saturating_sub(t);
                crate::conversation::turn_meta::format_duration(diff)
            })
        } else {
            None
        };

        let (effective_work_id, cancellable) = {
            let state = self.state.read(cx);
            state
                .active_conversation()
                .map(|c| c.subagent_work_control(work_id.as_deref(), child_session_id.as_deref()))
                .unwrap_or((work_id.clone(), true))
        };

        let stop_button = if is_running
            && cancellable
            && let Some(wid) = effective_work_id
        {
            self.activity_cancel_button(format!("stop-subagent-{r_id}"), wid, cx)
        } else {
            None
        };

        // Inside a subagent's transcript this card is its nested Explore, owned by
        // the viewed subagent rather than the main session.
        let owner = self.state.read(cx).subagent_open_owner();
        let open_button = child_session_id.as_ref().map(|sid| {
            let sid_clone = sid.clone();
            div()
                .id(ElementId::NamedInteger("open-subagent".into(), r_id))
                .px_2()
                .py_0p5()
                .rounded_sm()
                .bg(rgb(PANEL))
                .border_1()
                .border_color(rgb(BORDER))
                .text_size(px(ui_size(10.5)))
                .text_color(rgb(ACCENT))
                .hover(|h| h.bg(rgb(CARD_HOVER)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    if let Some((workspace, parent)) = &owner {
                        this.open_child_conversation(workspace, parent, &sid_clone, window, cx);
                    }
                }))
                .child("Open")
        });

        let id_chip = child_session_id
            .as_deref()
            .and_then(agent_id_from_child_session)
            .map(|agent_id| self.agent_id_copy_chip(agent_id, cx));

        let clamped = if !summary_text.is_empty() {
            let max_chars = 240;
            let text = if summary_text.chars().count() > max_chars {
                let prefix: String = summary_text.chars().take(max_chars).collect();
                format!("{prefix}...")
            } else {
                summary_text.clone()
            };
            Some(
                div()
                    .mt_1()
                    .text_size(px(ui_size(11.5)))
                    .text_color(rgb(MUTED))
                    .child(crate::shared::selectable::SelectableText::new(
                        ElementId::NamedInteger("subagent-summary".into(), r_id),
                        text,
                    )),
            )
        } else {
            None
        };

        let card_bg = if is_nested { PANEL } else { CARD };
        let mut card = div()
            .w_full()
            .flex()
            .flex_col()
            .bg(rgb(card_bg))
            .border_1()
            .border_color(rgb(BORDER))
            .rounded_md()
            .px_2p5()
            .py_1p5();

        if !is_nested {
            card = card.my_1();
        } else {
            card = card.mt_1();
        }

        card.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(7.)).h(px(7.)).rounded_full().bg(rgb(dot_color)))
                        .child(
                            div()
                                .font_family(crate::shared::theme::MONO_FONT)
                                .text_size(px(ui_size(12.)))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(TEXT))
                                .child(format!("{subagent_type} · {status}")),
                        )
                        .when_some(elapsed, |el, dur| {
                            el.child(
                                div()
                                    .text_size(px(ui_size(11.)))
                                    .text_color(rgb(MUTED))
                                    .child(format!("({dur})")),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .children(id_chip)
                        .children(stop_button)
                        .children(open_button),
                ),
        )
        .children(clamped)
        .into_any_element()
    }

    /// Render a navigation back bar when viewing a subagent child conversation.
    pub(crate) fn subagent_back_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.state.read(cx);
        let child_sid = state.viewing_child.as_ref()?.clone();
        let parent_title = state
            .parent_of_viewing_child()
            .and_then(|psid| {
                state
                    .workspaces
                    .iter()
                    .find_map(|w| {
                        w.sessions
                            .iter()
                            .find(|s| s.session_id == psid)
                            .map(|s| s.title.clone())
                    })
                    // A nested Explore's parent is a subagent, not a listed session.
                    .or_else(|| state.known_subagent_title(&psid))
            })
            .unwrap_or_else(|| "Parent conversation".into());

        Some(
            div()
                .id("subagent-back-bar")
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .px_3()
                .py_1p5()
                .bg(rgb(CARD))
                .border_1()
                .border_color(rgb(BORDER))
                .rounded_md()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .id("back-to-parent-btn")
                                .px_2()
                                .py_0p5()
                                .rounded_sm()
                                .bg(rgb(PANEL))
                                .border_1()
                                .border_color(rgb(BORDER))
                                .text_size(px(ui_size(11.5)))
                                .text_color(rgb(ACCENT))
                                .hover(|h| h.bg(rgb(CARD_HOVER)))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.close_child_conversation(window, cx);
                                }))
                                .child(format!("← Back to {parent_title}")),
                        )
                        .child(
                            div()
                                .text_size(px(ui_size(11.)))
                                .text_color(rgb(MUTED))
                                .child(format!("({child_sid})")),
                        ),
                )
                .child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .bg(rgb(PANEL))
                        .text_size(px(ui_size(10.5)))
                        .text_color(rgb(MUTED))
                        .child("read-only"),
                )
                .into_any_element(),
        )
    }

    /// Read-only placeholder card shown in place of the composer when in subagent view.
    pub(crate) fn subagent_read_only_banner(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("subagent-read-only-banner")
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .px_3()
            .py_2()
            .bg(rgb(CARD))
            .border_1()
            .border_color(rgb(BORDER))
            .rounded_md()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(6.5)).h(px(6.5)).rounded_full().bg(rgb(MUTED)))
                    .child(div().text_size(px(ui_size(12.))).text_color(rgb(MUTED)).child(
                        "Subagent session is read-only. Responses and commands are disabled.",
                    )),
            )
            .child(
                div()
                    .id("read-only-return-btn")
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(rgb(PANEL))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .text_size(px(ui_size(11.5)))
                    .text_color(rgb(ACCENT))
                    .hover(|h| h.bg(rgb(CARD_HOVER)))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.state.update(cx, |s, cx| s.close_subagent(cx));
                    }))
                    .child("Return to parent"),
            )
            .into_any_element()
    }
}
