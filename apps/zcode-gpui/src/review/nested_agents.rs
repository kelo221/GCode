//! Nested Explore agents of a subagent, rendered as indented rows under its
//! card in the Agents section (running) and under its row in Ended agents.
//! Data: `subagent_directories[<subagent child session>]`, filled by
//! `AppState::ensure_nested_subagents`.

use crate::app::root::RootView;
use crate::conversation::subagent_directory::DirectoryAgent;
use crate::conversation::turn_meta::format_duration;
use crate::review::agents_section::format_agent_title;
use crate::shared::theme::ui_size;
use crate::shared::theme::{
    ACCENT, BORDER, CARD_HOVER, DANGER, MUTED, PANEL, SUCCESS, TEXT, WARNING,
};
use crate::shared::theme_colors::color as rgb;
use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};

fn nested_status_color(status: &str) -> u32 {
    match status {
        "running" | "waiting" => ACCENT,
        "blocked" => WARNING,
        "failed" | "lost" => DANGER,
        "success" => SUCCESS,
        _ => MUTED,
    }
}

/// Built-in Explore agents cannot dispatch subagents of their own (the CLI
/// only lets non-Explore subagents start a foreground Explore), so they are
/// never polled for nested children.
pub(crate) fn may_nest(subagent_type: &str) -> bool {
    !subagent_type.trim().eq_ignore_ascii_case("explore")
}

/// Running first (oldest start first), then ended (newest end first).
pub(crate) fn nested_rows(
    running: &[DirectoryAgent],
    ended: &[DirectoryAgent],
) -> Vec<DirectoryAgent> {
    let mut running = running.to_vec();
    running.sort_by_key(|a| a.started_at);
    running.into_iter().chain(ended.iter().cloned()).collect()
}

impl RootView {
    /// Indented rows for the nested agents of `parent_sid` (a subagent of the
    /// active session); `None` when it has none.
    pub(crate) fn nested_agent_rows(
        &mut self,
        parent_sid: &str,
        parent_running: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.state.update(cx, |state, cx| {
            state.ensure_nested_subagents(parent_sid, parent_running, cx)
        });
        let state = self.state.read(cx);
        let workspace = state.active_ws_key()?;
        let cache = state.ws(&workspace)?.subagent_directories.get(parent_sid)?;
        let rows = nested_rows(&cache.running, &cache.items);
        if rows.is_empty() {
            return None;
        }
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut list = div().min_w_0().flex().flex_col().gap_0p5().pl(px(6.));
        for agent in rows {
            let running = agent.ended_at.is_none()
                && matches!(agent.status.as_str(), "running" | "waiting" | "blocked");
            let duration = match (agent.started_at, agent.ended_at) {
                (Some(start), Some(end)) => Some(format_duration(end.saturating_sub(start))),
                (Some(start), None) if running => {
                    Some(format_duration(now_ms.saturating_sub(start)))
                }
                _ => None,
            };
            let child = agent.child_session_id.clone();
            let parent = parent_sid.to_string();
            let workspace = workspace.clone();
            let open = div()
                .id(gpui::SharedString::from(format!("nested-open-{child}")))
                .flex_none()
                .px_1p5()
                .rounded_sm()
                .bg(rgb(PANEL))
                .border_1()
                .border_color(rgb(BORDER))
                .text_size(px(ui_size(10.)))
                .text_color(rgb(ACCENT))
                .hover(|h| h.bg(rgb(CARD_HOVER)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.open_child_conversation(&workspace, &parent, &child, window, cx);
                }))
                .child(crate::shared::i18n::label("Open", "打开"));
            #[cfg(test)]
            let open = crate::app::test_support::track_children(
                div().child(open),
                vec![format!("nested-open-{}", agent.child_session_id)],
            );
            let title = format_agent_title(&agent.title, &agent.subagent_type);
            list = list.child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .text_size(px(ui_size(10.5)))
                    .child(div().flex_none().text_color(rgb(MUTED)).child("└"))
                    .child(
                        div()
                            .flex_none()
                            .w(px(5.5))
                            .h(px(5.5))
                            .rounded_full()
                            .bg(rgb(nested_status_color(&agent.status))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(crate::shared::theme::MONO_FONT)
                            .text_color(rgb(TEXT))
                            .child(format!("{} · {title}", agent.subagent_type)),
                    )
                    .children(duration.map(|d| div().flex_none().text_color(rgb(MUTED)).child(d)))
                    .child(open),
            );
        }
        Some(list.into_any_element())
    }
}
