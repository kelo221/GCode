//! Agents dock section: the running subagents (cards in review/agent_card.rs:
//! model, elapsed time, current todo, Stop/Open) and the ended-agents list.

use crate::app::root::RootView;
use crate::composer::catalog::ModelOption;
use crate::shared::theme::ui_size;
use crate::shared::theme::{ACCENT, BORDER, HOVER, MUTED, TEXT};
use crate::shared::theme_colors::color as rgb;
use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use std::collections::HashMap;

/// Format title or type fallback for an agent item.
pub fn format_agent_title<'a>(title: &'a str, subagent_type: &'a str) -> &'a str {
    let t = title.trim();
    if !t.is_empty() { t } else { subagent_type }
}

/// Format ended subagent count summary label.
pub fn format_ended_summary(ended_total: u64) -> Option<String> {
    if ended_total == 0 {
        None
    } else {
        Some(format!(
            "{ended_total} ended subagent{}",
            if ended_total == 1 { "" } else { "s" }
        ))
    }
}

impl RootView {
    /// Render the collapsible Agents section for the review dock pane.
    pub(crate) fn render_agents_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let ended = self.render_ended_agents(cx);
        // Main view only: inside a subagent the running list already is its
        // nested children (from the subagent's own projection).
        let running_ids: Vec<String> = {
            let state = self.state.read(cx);
            match state.active_conversation() {
                Some(conv) if self.agents_expanded && !state.is_read_only_view() => conv
                    .running_subagents()
                    .into_iter()
                    .filter(|a| crate::review::nested_agents::may_nest(&a.subagent_type))
                    .map(|a| a.child_session_id)
                    .collect(),
                _ => Vec::new(),
            }
        };
        let mut nested: HashMap<String, AnyElement> = HashMap::new();
        for sid in running_ids {
            if let Some(rows) = self.nested_agent_rows(&sid, true, cx) {
                nested.insert(sid, rows);
            }
        }
        let state = self.state.read(cx);
        let conv = state.active_conversation();
        let mut running = conv.map(|c| c.running_subagents()).unwrap_or_default();
        running.sort_by(|a, b| {
            a.started_at
                .cmp(&b.started_at)
                .then(a.child_session_id.cmp(&b.child_session_id))
        });
        // While viewing a subagent, these cards are its nested Explore children.
        let owner = state.subagent_open_owner();
        let models: Vec<ModelOption> = state
            .active_ws_key()
            .and_then(|k| state.workspace_configs.get(&k))
            .map(|c| c.models.clone())
            .unwrap_or_default();
        let running_count = running.len();
        let ended_count = conv
            .and_then(|c| c.subagents.as_ref())
            .map(|s| s.ended_total)
            .unwrap_or(0);

        let expanded = self.agents_expanded;
        let counter_color = if running_count > 0 { ACCENT } else { MUTED };

        let header = div()
            .id("agents-section-header")
            .h(px(32.))
            .min_w_0()
            .flex()
            .items_center()
            .gap_1p5()
            .px_3()
            .cursor_pointer()
            .hover(|s| s.bg(rgb(HOVER)))
            .rounded_sm()
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.agents_expanded = !this.agents_expanded;
                cx.notify();
            }))
            .child(
                div()
                    .text_size(px(ui_size(12.)))
                    .text_color(rgb(TEXT))
                    .child("Agents"),
            )
            .child(
                div()
                    .text_size(px(ui_size(11.)))
                    .text_color(rgb(MUTED))
                    .child(if expanded { "▾" } else { "▸" }),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(ui_size(12.)))
                    .text_color(rgb(counter_color))
                    .child(format!("{running_count}")),
            );

        let content = if running.is_empty() {
            let msg = if ended_count > 0 {
                format!("No running subagents ({ended_count} ended)")
            } else {
                "No running subagents".to_string()
            };
            div()
                .px_2()
                .py_2()
                .text_size(px(ui_size(11.5)))
                .text_color(rgb(MUTED))
                .child(msg)
                .into_any_element()
        } else {
            let mut list = div()
                .id("agents-running-list")
                .flex()
                .flex_col()
                .gap_1p5()
                .px_2()
                .pb_1p5();

            for (idx, agent) in running.iter().enumerate() {
                let rows = nested.remove(&agent.child_session_id);
                list = list.child(self.running_agent_card(
                    idx,
                    agent,
                    owner.clone(),
                    &models,
                    rows,
                    cx,
                ));
            }

            if let Some(ended_msg) = format_ended_summary(ended_count) {
                list = list.child(
                    div()
                        .px_2()
                        .pt_1()
                        .text_size(px(ui_size(10.5)))
                        .text_color(rgb(MUTED))
                        .child(ended_msg),
                );
            }

            list.into_any_element()
        };

        div()
            .w_full()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(rgb(BORDER))
            .pt_1()
            .child(header)
            .when(expanded, |el| el.child(content))
            .children(ended)
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "agents_section_tests.rs"]
mod tests;
