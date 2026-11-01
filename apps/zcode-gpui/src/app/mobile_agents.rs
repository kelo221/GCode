//! Phone chat header subagent icons: one numbered bot icon per subagent of
//! the active task, since the phone has no Agents dock.
//! Spec: REMOTE_CONTROL.md "Phone layout", Subagent icons.

use crate::app::root::RootView;
use crate::conversation::subagent_directory::DirectoryAgent;
use crate::conversation::subagents::JoinedRunningSubagent;
use crate::shared::theme::{ACCENT, DANGER, HOVER, MUTED, SELECTED, TEXT, icon, ui_size};
use crate::shared::theme_colors::color as rgb;
use ely_gpui_component::buttons::ButtonVariant;
use ely_gpui_component::menus::{DropdownMenu, Menu, MenuItem};
use ely_gpui_component::primitives::IconName;
use gpui::{AnyElement, ClickEvent, Context, div, prelude::*, px};

/// Icons shown before the rest go into the "+N" menu.
pub(crate) const MAX_ICONS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgentDot {
    Running,
    Failed,
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HeaderAgent {
    pub child: String,
    pub number: usize,
    pub title: String,
    pub status: String,
    pub dot: AgentDot,
    pub viewing: bool,
    started_at: Option<u64>,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct HeaderAgents {
    /// In number order.
    pub shown: Vec<HeaderAgent>,
    /// In number order; listed by the "+N" menu.
    pub overflow: Vec<HeaderAgent>,
}

fn dot_for(status: &str) -> AgentDot {
    match status {
        "running" | "waiting" | "blocked" => AgentDot::Running,
        "failed" | "lost" => AgentDot::Failed,
        _ => AgentDot::None,
    }
}

/// Number running + ended agents by start time and pick which ones get icons.
pub(crate) fn header_agents(
    running: &[JoinedRunningSubagent],
    ended: &[DirectoryAgent],
    viewing: Option<&str>,
) -> HeaderAgents {
    let mut all: Vec<HeaderAgent> = Vec::new();
    let mut push = |child: &str, title: &str, kind: &str, status: &str, started: Option<u64>| {
        // The live projection wins over a directory row for the same child.
        if all.iter().any(|a| a.child == child) {
            return;
        }
        let title = if title.trim().is_empty() { kind } else { title };
        all.push(HeaderAgent {
            child: child.to_string(),
            number: 0,
            title: title.trim().to_string(),
            status: status.to_string(),
            dot: dot_for(status),
            viewing: viewing == Some(child),
            started_at: started,
        });
    };
    for a in running {
        push(
            &a.child_session_id,
            &a.title,
            &a.subagent_type,
            &a.status,
            a.started_at,
        );
    }
    for a in ended {
        push(
            &a.child_session_id,
            &a.title,
            &a.subagent_type,
            &a.status,
            a.started_at,
        );
    }
    all.sort_by(|a, b| {
        (a.started_at.is_none(), a.started_at, &a.child).cmp(&(
            b.started_at.is_none(),
            b.started_at,
            &b.child,
        ))
    });
    for (index, agent) in all.iter_mut().enumerate() {
        agent.number = index + 1;
    }
    // Priority: the open child, running agents, then the newest ended ones.
    let mut by_priority: Vec<usize> = (0..all.len()).collect();
    by_priority.sort_by_key(|&i| {
        let a = &all[i];
        (
            !a.viewing,
            a.dot != AgentDot::Running,
            std::cmp::Reverse(a.number),
        )
    });
    let keep: std::collections::HashSet<usize> = by_priority.into_iter().take(MAX_ICONS).collect();
    let mut out = HeaderAgents::default();
    for (index, agent) in all.into_iter().enumerate() {
        if keep.contains(&index) {
            out.shown.push(agent);
        } else {
            out.overflow.push(agent);
        }
    }
    out
}

impl RootView {
    /// The icon row for the phone chat header; None when the task has no subagents.
    pub(crate) fn mobile_agent_icons(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.state
            .update(cx, |s, cx| s.ensure_subagent_directory(false, false, cx));
        let state = self.state.read(cx);
        let (workspace, parent) = state.active_ws_key().zip(state.active.clone())?;
        let running = state
            .conversations
            .get(&parent)
            .map(|c| c.running_subagents())
            .unwrap_or_default();
        let ended = state
            .ws(&workspace)
            .and_then(|ws| ws.subagent_directories.get(&parent))
            .map(|d| d.items.as_slice())
            .unwrap_or(&[]);
        let agents = header_agents(&running, ended, state.viewing_child.as_deref());
        if agents.shown.is_empty() {
            return None;
        }
        let mut row = div().flex().flex_none().items_center().gap_1();
        #[cfg(test)]
        let mut tracked = Vec::new();
        for agent in agents.shown {
            let id = format!("mobile-agent-{}", agent.number);
            #[cfg(test)]
            tracked.push(id.clone());
            let (workspace, parent, child) =
                (workspace.clone(), parent.clone(), agent.child.clone());
            let viewing = agent.viewing;
            let tint = if agent.dot == AgentDot::None && !viewing {
                MUTED
            } else {
                TEXT
            };
            let dot = match agent.dot {
                AgentDot::Running => Some(ACCENT),
                AgentDot::Failed => Some(DANGER),
                AgentDot::None => None,
            };
            row = row.child(
                div()
                    .id(gpui::SharedString::from(id))
                    .relative()
                    .h(px(32.))
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(viewing, |el| el.bg(rgb(SELECTED)))
                    .hover(|s| s.bg(rgb(HOVER)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        if viewing {
                            this.close_child_conversation(window, cx);
                        } else {
                            this.open_child_conversation(&workspace, &parent, &child, window, cx);
                        }
                    }))
                    .child(icon(IconName::Bot, 14., tint))
                    .child(
                        div()
                            .text_size(px(ui_size(12.)))
                            .text_color(rgb(tint))
                            .child(agent.number.to_string()),
                    )
                    .children(dot.map(|c| {
                        div()
                            .absolute()
                            .top(px(5.))
                            .right(px(3.))
                            .size(px(6.))
                            .rounded_full()
                            .bg(rgb(c))
                    })),
            );
        }
        if !agents.overflow.is_empty() {
            #[cfg(test)]
            tracked.push("mobile-agent-more".into());
            let root = cx.entity();
            let mut menu = Menu::new();
            for agent in agents.overflow.iter() {
                let (root, workspace, parent, child) = (
                    root.clone(),
                    workspace.clone(),
                    parent.clone(),
                    agent.child.clone(),
                );
                let label = format!("{}  {} · {}", agent.number, agent.title, agent.status);
                menu = menu.item(MenuItem::new(label).on_click(move |window, cx| {
                    root.update(cx, |this, cx| {
                        this.open_child_conversation(&workspace, &parent, &child, window, cx)
                    });
                }));
            }
            row = row.child(
                DropdownMenu::new(
                    "mobile-agent-more",
                    format!("+{}", agents.overflow.len()),
                    menu,
                )
                .variant(ButtonVariant::Ghost),
            );
        }
        #[cfg(test)]
        let row = crate::app::test_support::track_children(row, tracked);
        Some(row.into_any_element())
    }
}

#[cfg(test)]
#[path = "mobile_agents_tests.rs"]
mod tests;
