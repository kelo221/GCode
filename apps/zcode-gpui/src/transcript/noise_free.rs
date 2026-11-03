//! Noise-free transcript (PARITY.md "Noise-free transcript", GPUI only): agent
//! text, thinking and todo updates stay visible; each run of other tool calls
//! between them folds into one muted line that expands on click.

use crate::app::root::RootView;
use crate::conversation::model::Row;
use crate::shared::i18n::label;
use crate::shared::theme::{DANGER, MUTED, ui_size};
use crate::shared::theme_colors::color as rgb;
use crate::transcript::tool_inputs::canonical_tool;
use ely_gpui_component::motion::{Spinner, SpinnerStyle};
use ely_gpui_component::theme::IconSize;
use gpui::{AnyElement, App, Context, CursorStyle, IntoElement, div, prelude::*, px};

/// How a row takes part in folding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Kind {
    /// Always shown; ends a fold run.
    Keep,
    /// Folded into the surrounding run.
    Fold {
        running: bool,
        failed: bool,
        /// Edited file and its error count, from the edit's diagnostics.
        diagnostics: Option<(u64, u32)>,
    },
    /// Renders nothing on its own (empty thinking, paired subagent rows):
    /// neither shown nor a run boundary.
    Transparent,
}

/// Tools whose cards carry what the user is following: the todo list, the
/// questions asked of them, the plan, and the sentence the agent said aloud.
const KEPT_TOOLS: [&str; 4] = ["TodoWrite", "AskUserQuestion", "ExitPlanMode", "Speak"];

pub(crate) fn kind(row: &Row) -> Kind {
    match row {
        Row::TurnHeader { .. } | Row::UserInput { .. } => Kind::Keep,
        Row::AssistantText { text, .. } if !text.trim().is_empty() => Kind::Keep,
        Row::AssistantText { .. } => Kind::Transparent,
        Row::Reasoning { text, .. } if crate::transcript::reasoning::has_reasoning_text(text) => {
            Kind::Keep
        }
        Row::Reasoning { .. } | Row::Other => Kind::Transparent,
        Row::ToolCall {
            tool_name,
            status,
            display,
            ..
        } => {
            if status == "pendingApproval" || KEPT_TOOLS.contains(&canonical_tool(tool_name)) {
                Kind::Keep
            } else {
                Kind::Fold {
                    running: status == "running" || status == "inputStreaming",
                    failed: status == "error",
                    diagnostics: edit_errors(display.as_ref()),
                }
            }
        }
        // Paired subagents render inside their Agent tool card.
        Row::Subagent {
            parent_tool_call_id: Some(_),
            ..
        } => Kind::Transparent,
        Row::Subagent { status, .. } => Kind::Fold {
            running: status == "running",
            failed: status == "failed",
            diagnostics: None,
        },
    }
}

/// A finished turn that drew nothing (a compaction, which GPUI renders no
/// marker for): its header would be a bare "Worked for 0s" divider.
/// Desktop shows the work status only above assistant content.
pub(crate) fn is_empty_finished_turn(
    rows: &std::collections::BTreeMap<u64, Row>,
    header_id: u64,
) -> bool {
    let Some(Row::TurnHeader {
        state,
        file_changes,
        ..
    }) = rows.get(&header_id)
    else {
        return false;
    };
    if matches!(
        state.as_str(),
        "running" | "failed" | "completedInterrupted"
    ) || file_changes.is_some()
    {
        return false;
    }
    rows.range(header_id + 1..)
        .take_while(|(_, r)| !matches!(r, Row::TurnHeader { .. }))
        .all(|(_, r)| kind(r) == Kind::Transparent)
}

fn edit_errors(display: Option<&serde_json::Value>) -> Option<(u64, u32)> {
    let diagnostics = crate::transcript::edit_diagnostics::edit_diagnostics(display)?;
    if !diagnostics.available {
        return None;
    }
    Some((
        crate::transcript::edit_diagnostics::file_key(display)?,
        diagnostics.errors,
    ))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Summary {
    pub count: usize,
    pub running: usize,
    pub failed: usize,
    /// Errors in the files edited in this run, latest result per file.
    pub errors: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Place {
    Show,
    /// Folded row whose run starts at list index `leader`.
    Member {
        leader: usize,
    },
    /// First row of a run: renders the fold line.
    Leader(Summary),
}

/// Where list row `ix` sits. Only walks its own run, so a render pass costs
/// O(run length) per visible row, and counts follow live status changes.
pub(crate) fn place(ix: usize, len: usize, kind_at: impl Fn(usize) -> Kind) -> Place {
    if !matches!(kind_at(ix), Kind::Fold { .. }) {
        return Place::Show;
    }
    let mut leader = ix;
    for i in (0..ix).rev() {
        match kind_at(i) {
            Kind::Keep => break,
            Kind::Fold { .. } => leader = i,
            Kind::Transparent => {}
        }
    }
    if leader != ix {
        return Place::Member { leader };
    }
    let mut summary = Summary::default();
    // A later edit of the same file replaces its count, so fixed errors drop out.
    let mut latest: Vec<(u64, u32)> = Vec::new();
    for i in ix..len {
        match kind_at(i) {
            Kind::Keep => break,
            Kind::Fold {
                running,
                failed,
                diagnostics,
            } => {
                summary.count += 1;
                summary.running += usize::from(running);
                summary.failed += usize::from(failed);
                if let Some((file, errors)) = diagnostics {
                    match latest.iter_mut().find(|(key, _)| *key == file) {
                        Some(entry) => entry.1 = errors,
                        None => latest.push((file, errors)),
                    }
                }
            }
            Kind::Transparent => {}
        }
    }
    summary.errors = latest.iter().map(|(_, errors)| *errors as usize).sum();
    Place::Leader(summary)
}

pub(crate) fn enabled(cx: &App) -> bool {
    cx.global::<crate::shared::preferences::PreferenceOwner>()
        .0
        .read(cx)
        .snapshot
        .noise_free()
}

fn summary_text(summary: Summary, expanded: bool) -> String {
    let calls = match (summary.count, expanded) {
        (1, false) => label("1 tool call", "1 个工具调用").to_owned(),
        (1, true) => label("Hide 1 tool call", "收起 1 个工具调用").to_owned(),
        (n, false) => format!("{n} {}", label("tool calls", "个工具调用")),
        (n, true) => format!(
            "{} {n} {}",
            label("Hide", "收起"),
            label("tool calls", "个工具调用")
        ),
    };
    let mut text = calls;
    if summary.running > 0 {
        text.push_str(&format!(
            " · {} {}",
            summary.running,
            label("running", "运行中")
        ));
    }
    text
}

impl RootView {
    pub(crate) fn fold_place(&self, ix: usize, cx: &App) -> Place {
        let state = self.state.read(cx);
        let Some(conversation) = state.active_conversation() else {
            return Place::Show;
        };
        let kind_at = |i: usize| {
            self.row_index
                .get(i)
                .and_then(|id| conversation.rows.get(id))
                .map_or(Kind::Transparent, kind)
        };
        place(ix, self.row_index.len(), kind_at)
    }

    pub(crate) fn render_fold_line(
        &self,
        leader_id: u64,
        summary: Summary,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let line = div()
            .id(("noise-fold", leader_id as usize))
            .flex()
            .items_center()
            .gap_2()
            .py_1()
            .text_size(px(ui_size(12.)))
            .text_color(rgb(MUTED))
            .cursor(CursorStyle::PointingHand)
            .hover(|s| s.opacity(0.8))
            .child(crate::shared::theme::icon(
                if expanded {
                    crate::shared::theme::I_CHEVRON_DOWN
                } else {
                    crate::shared::theme::I_CHEVRON_RIGHT
                },
                11.,
                MUTED,
            ))
            .child(summary_text(summary, expanded))
            .when(summary.running > 0, |el| {
                el.child(
                    Spinner::new(("noise-fold-spinner", leader_id as usize))
                        .style(SpinnerStyle::Ring)
                        .size(IconSize::Sm),
                )
            })
            .when(summary.errors > 0, |el| {
                el.child(div().text_color(rgb(DANGER)).child(match summary.errors {
                    1 => label("1 error", "1 个错误").to_owned(),
                    n => format!("{n} {}", label("errors", "个错误")),
                }))
            })
            .when(summary.failed > 0, |el| {
                el.child(div().text_color(rgb(DANGER)).child(format!(
                    "{} {}",
                    summary.failed,
                    label("failed", "失败")
                )))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.expanded_folds.remove(&leader_id) {
                    this.expanded_folds.insert(leader_id);
                }
                cx.notify();
            }));
        let line = div().child(line);
        #[cfg(test)]
        let line =
            crate::app::test_support::track_children(line, vec![format!("noise-fold-{leader_id}")]);
        line.into_any_element()
    }
}

#[cfg(test)]
#[path = "noise_free_tests.rs"]
mod tests;
