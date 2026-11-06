//! One running-subagent card in the Review dock: title, the model the child
//! runs on, elapsed time, its current todo with a `completed/total` counter,
//! and Stop / Open. Split from review/agents_section.rs for the 400-line cap.

use crate::app::root::RootView;
use crate::composer::catalog::ModelOption;
use crate::conversation::subagents::JoinedRunningSubagent;
use crate::conversation::turn_meta::PlanItem;
use crate::review::agents_section::format_agent_title;
use crate::shared::theme::ui_size;
use crate::shared::theme::{
    ACCENT, BORDER, CARD, CARD_HOVER, DANGER, MUTED, PANEL, SUCCESS, TEXT, WARNING,
};
use crate::shared::theme_colors::color as rgb;
use gpui::{
    AnyElement, Context, ElementId, IntoElement, ParentElement, Styled, div, prelude::*, px,
    relative,
};

/// Display name for a child's `provider/model`: the catalog name when the
/// workspace knows the model, else the bare model id.
pub fn model_label(raw: &str, models: &[ModelOption]) -> String {
    let raw = raw.trim();
    let (provider, model) = raw.split_once('/').unwrap_or(("", raw));
    models
        .iter()
        .find(|m| m.model == model && (provider.is_empty() || m.provider == provider))
        .map(|m| m.name.trim())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| model.to_string())
}

#[derive(Clone, Debug, PartialEq)]
pub struct TodoProgress {
    /// The in-progress todo, else the next pending one; `None` once all are done.
    pub current: Option<String>,
    pub completed: usize,
    pub total: usize,
}

pub fn todo_progress(todos: &[PlanItem]) -> Option<TodoProgress> {
    if todos.is_empty() {
        return None;
    }
    let completed = todos.iter().filter(|t| t.status == "completed").count();
    let current = todos
        .iter()
        .find(|t| t.status == "inProgress")
        .or_else(|| todos.iter().find(|t| t.status == "pending"))
        .map(|t| t.content.trim().to_string())
        .filter(|c| !c.is_empty());
    Some(TodoProgress {
        current,
        completed,
        total: todos.len(),
    })
}

/// One-line preview of a markdown report: the first line with text, without
/// heading/list/quote markers or emphasis, cut at `max_chars`.
pub fn summary_preview(summary: Option<&str>, max_chars: usize) -> Option<String> {
    let line = summary?
        .lines()
        .map(|l| l.trim().trim_start_matches(['#', '>', '-', '*', '+', ' ']))
        .find(|l| l.chars().any(char::is_alphanumeric))?;
    let text = line
        .replace("**", "")
        .replace('`', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if text.chars().count() > max_chars {
        let prefix: String = text.chars().take(max_chars).collect();
        Some(format!("{}…", prefix.trim_end()))
    } else {
        Some(text)
    }
}

fn status_color(status: &str) -> u32 {
    match status {
        "running" => ACCENT,
        "blocked" => WARNING,
        "failed" | "error" => DANGER,
        "success" | "completed" => SUCCESS,
        _ => MUTED,
    }
}

fn todo_row(progress: &TodoProgress) -> gpui::Div {
    let done = progress.current.is_none();
    let (marker, color) = if done {
        ("✓", SUCCESS)
    } else {
        ("●", ACCENT)
    };
    let text = progress
        .current
        .clone()
        .unwrap_or_else(|| "All todos done".to_string());
    let ratio = progress.completed as f32 / progress.total.max(1) as f32;
    div()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_1p5()
                .text_size(px(ui_size(11.)))
                .child(div().flex_none().text_color(rgb(color)).child(marker))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(rgb(if done { MUTED } else { TEXT }))
                        .child(text),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(rgb(if done { SUCCESS } else { MUTED }))
                        .child(format!("{}/{}", progress.completed, progress.total)),
                ),
        )
        .child(
            div()
                .h(px(2.))
                .w_full()
                .rounded_full()
                .bg(rgb(BORDER))
                .child(
                    div()
                        .h_full()
                        .w(relative(ratio))
                        .rounded_full()
                        .bg(rgb(color)),
                ),
        )
}

impl RootView {
    pub(crate) fn running_agent_card(
        &self,
        idx: usize,
        agent: &JoinedRunningSubagent,
        owner: Option<(String, String)>,
        models: &[ModelOption],
        nested: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = format_agent_title(&agent.title, &agent.subagent_type);
        let elapsed = agent.started_at.map(|t| {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(t);
            crate::conversation::turn_meta::format_duration(now_ms.saturating_sub(t))
        });
        let stop_button = agent
            .work_id
            .clone()
            .filter(|_| agent.cancellable)
            .and_then(|wid| {
                self.activity_cancel_button(format!("dock-stop-subagent-{idx}"), wid, cx)
            });
        let open_button = (!agent.child_session_id.is_empty()).then(|| {
            let child = agent.child_session_id.clone();
            let element = div().child(
                div()
                    .id(ElementId::NamedInteger(
                        "dock-open-subagent".into(),
                        idx as u64,
                    ))
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
                            this.open_child_conversation(workspace, parent, &child, window, cx);
                        }
                    }))
                    .child(crate::shared::i18n::label("Open", "打开")),
            );
            #[cfg(test)]
            let element = crate::app::test_support::track_children(
                element,
                vec![format!("agent-open-{}", agent.child_session_id)],
            );
            element
        });
        let id_chip = agent
            .agent_id
            .as_deref()
            .filter(|id| id.starts_with("agent_"))
            .or_else(|| {
                crate::transcript::subagent_card::agent_id_from_child_session(
                    &agent.child_session_id,
                )
            })
            .map(|agent_id| self.agent_id_copy_chip(agent_id, cx));
        let model_chip = agent.model.as_deref().map(|raw| {
            let chip = div().flex_none().child(
                div()
                    .px_1p5()
                    .rounded_sm()
                    .bg(rgb(PANEL))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .text_size(px(ui_size(10.5)))
                    .text_color(rgb(TEXT))
                    .child(model_label(raw, models)),
            );
            #[cfg(test)]
            let chip = crate::app::test_support::track_children(
                chip,
                vec![format!("agent-model-{}", agent.child_session_id)],
            );
            chip
        });
        let todo = todo_progress(&agent.todos).map(|progress| {
            let row = todo_row(&progress);
            #[cfg(test)]
            let row = crate::app::test_support::track_children(
                div().child(row),
                vec![format!("agent-todo-{}", agent.child_session_id)],
            );
            row
        });
        let summary = summary_preview(agent.summary.as_deref(), 160).map(|text| {
            div()
                .line_clamp(2)
                .text_ellipsis()
                .text_size(px(ui_size(11.)))
                .text_color(rgb(MUTED))
                .child(text)
        });

        // Desktop `SubagentStatusSection`: the title gets its own line-clamped
        // block and the meta/controls row sits below it, so a long title can
        // never push Stop / Open out of the card.
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1()
            .bg(rgb(CARD))
            .border_1()
            .border_color(rgb(BORDER))
            .rounded_md()
            .p_1p5()
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_start()
                    .gap_1p5()
                    .child(
                        div()
                            .flex_none()
                            .mt(px(5.))
                            .w(px(6.5))
                            .h(px(6.5))
                            .rounded_full()
                            .bg(rgb(status_color(&agent.status))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .line_clamp(2)
                            .text_ellipsis()
                            .font_family(crate::shared::theme::MONO_FONT)
                            .text_size(px(ui_size(11.5)))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT))
                            .child(title.to_string()),
                    ),
            )
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .pl(px(12.5))
                    .children(model_chip)
                    .when_some(elapsed, |el, dur| {
                        el.child(
                            div()
                                .text_size(px(ui_size(10.5)))
                                .text_color(rgb(MUTED))
                                .child(dur),
                        )
                    })
                    .child(div().flex_1())
                    .children(id_chip)
                    .children(stop_button)
                    .children(open_button),
            )
            .children(todo.map(|row| div().pl(px(12.5)).child(row)))
            .children(summary.map(|node| node.pl(px(12.5))))
            .children(nested.map(|rows| div().pl(px(6.5)).child(rows)))
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "agent_card_tests.rs"]
mod tests;
