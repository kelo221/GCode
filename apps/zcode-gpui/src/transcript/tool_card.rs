//! Rendering of tool execution rows: a collapsed row with a human summary of
//! the call (desktop parity — the raw JSON is a last resort, not the default),
//! expandable to per-tool input views (Edit renders as a diff) and outputs.
use crate::shared::theme::ui_size;
use crate::shared::theme_colors::color as rgb;

use crate::conversation::model::format_preview;
use crate::shared::theme::{
    BG, BORDER, DANGER, I_CHEVRON_DOWN, I_CHEVRON_RIGHT, MONO_FONT, MUTED, SUBTLEST, TAG, TEXT,
    UI_FONT, icon,
};
use crate::shared::theme_colors::tinted;
use crate::transcript::tagged_text::clean_tool_output;
use crate::transcript::tool_inputs::{
    canonical_tool, generic_input_view, mono, selectable, task_output_view,
};
use crate::transcript::tool_results::displayed_result;
use ely_gpui_component::primitives::IconName;
use gpui::{
    AnyElement, Context, ElementId, IntoElement, ParentElement, Styled, div, prelude::*, px,
};
use serde_json::Value;

const TOOL_GROUP: &str = "tool-summary";

/// Borrowed view of a `Row::ToolCall` for rendering.
pub(crate) struct ToolCallView<'a> {
    pub row_id: u64,
    pub label: &'a str,
    pub tool_name: &'a str,
    pub status: &'a str,
    pub input_text: &'a str,
    pub output_text: &'a str,
    pub display: Option<&'a Value>,
}

/// Desktop renderer icon per tool kind (`ToolCallBlocks/renderers/*`).
fn tool_icon(label: &str) -> IconName {
    match label {
        "Read" | "Grep" | "Glob" | "LS" | "FindFiles" | "SearchCode" => IconName::Search,
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => IconName::Pencil,
        "Bash" => IconName::Terminal,
        "WebFetch" | "WebSearch" => IconName::Globe,
        "Task" | "Agent" | "CreateWorkflow" => IconName::Bot,
        "TodoWrite" | "TodoRead" => IconName::ListTodo,
        "AskUserQuestion" => IconName::CircleHelp,
        "ExitPlanMode" => IconName::ListChecks,
        "Speak" => IconName::Volume2,
        _ => IconName::Wrench,
    }
}

impl crate::app::root::RootView {
    pub(crate) fn render_tool_row(
        &self,
        view: ToolCallView<'_>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ToolCallView {
            row_id: r_id,
            label,
            tool_name,
            status,
            input_text,
            output_text,
            display,
        } = view;
        let tool = canonical_tool(if tool_name.is_empty() {
            label
        } else {
            tool_name
        });
        let expanded = self.expanded_tools.contains(&r_id);
        let is_running = status == "running" || status == "inputStreaming";
        let is_error = status == "error";
        let input: Option<Value> = serde_json::from_str(input_text).ok();
        let images = self.tool_images_view(
            r_id,
            crate::transcript::tool_images::image_refs(tool, status, input.as_ref(), display),
            cx,
        );

        div()
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id(ElementId::NamedInteger("toggle-tool".into(), r_id))
                    .group(TOOL_GROUP)
                    .max_w_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(ui_size(14.)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if !this.expanded_tools.remove(&r_id) {
                            this.expanded_tools.insert(r_id);
                        }
                        cx.notify();
                    }))
                    .child(icon(tool_icon(tool), 16., MUTED))
                    .child(
                        div()
                            .flex_none()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(rgb(if is_running { MUTED } else { SUBTLEST }))
                            .child(label.to_string()),
                    )
                    .child(summary_label(tool, input.as_ref()))
                    .children(crate::transcript::edit_diagnostics::diagnostics_chip(
                        display, r_id,
                    ))
                    .when(is_error, |el| {
                        el.child(
                            div()
                                .flex_none()
                                .text_color(rgb(DANGER))
                                .child(crate::shared::i18n::label("Failed", "失败")),
                        )
                    })
                    .when(tool == "CreateWorkflow" || tool == "Agent", |chip| {
                        chip.child(
                            div()
                                .id(ElementId::NamedInteger("subagent-badge".into(), r_id))
                                .flex_none()
                                .px_1p5()
                                .py_0p5()
                                .rounded_full()
                                .bg(tinted(TAG, 0.5))
                                .text_size(px(ui_size(12.)))
                                .text_color(rgb(MUTED))
                                .cursor_pointer()
                                .hover(|h| h.text_color(rgb(TEXT)))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.dock_open = true;
                                    this.dock_tab = crate::app::dock::DockTab::Workflows;
                                    cx.notify();
                                }))
                                .child("Subagents"),
                        )
                    })
                    .child(
                        div()
                            .flex_none()
                            .when(!expanded, |el| {
                                el.opacity(0.).group_hover(TOOL_GROUP, |s| s.opacity(1.))
                            })
                            .child(icon(
                                if expanded {
                                    I_CHEVRON_DOWN
                                } else {
                                    I_CHEVRON_RIGHT
                                },
                                16.,
                                SUBTLEST,
                            )),
                    ),
            )
            .when(expanded, |el| {
                el.child(
                    div().pt_2().child(
                        div()
                            // Scopes the selectable text ids to this card.
                            .id(ElementId::NamedInteger("tool-body".into(), r_id))
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .bg(rgb(BG))
                            .w_full()
                            .min_w_0()
                            .child(input_view(tool, input.as_ref(), input_text, r_id))
                            .child(output_view(tool, display, output_text)),
                    ),
                )
            })
            .children(images.map(|el| div().pt_2().w_full().child(el)))
            .into_any_element()
    }
}

/// Trailing summary in the collapsed row; empty when the input is not JSON we
/// understand (the row then reads label + status only, never raw JSON).
fn summary_label(label: &str, input: Option<&Value>) -> AnyElement {
    let text = input.and_then(|v| tool_summary(label, v));
    let Some(text) = text else {
        return div().into_any_element();
    };
    div()
        .font_family(UI_FONT)
        .text_color(rgb(MUTED))
        .min_w_0()
        .truncate()
        .child(format_preview(&text, 80))
        .into_any_element()
}

/// One-line human summary per tool (desktop parity: primary param inline).
pub(crate) fn tool_summary(label: &str, v: &Value) -> Option<String> {
    let str_field = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    match label {
        "Bash" => str_field("command").map(|c| format!("$ {c}")),
        "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => str_field("file_path")
            .or_else(|| str_field("notebook_path"))
            .or_else(|| str_field("path"))
            .map(short_path),
        "Grep" => Some(
            str_field("pattern")
                .map(|p| format!("\"{p}\""))
                .unwrap_or_default(),
        ),
        "Glob" => str_field("pattern"),
        "FindFiles" => str_field("query"),
        "SearchCode" => str_field("query").map(|q| format!("\"{q}\"")).or_else(|| {
            let patterns = v
                .get("patterns")?
                .as_array()?
                .iter()
                .filter_map(Value::as_str);
            Some(patterns.collect::<Vec<_>>().join(" | "))
        }),
        "WebFetch" => str_field("url"),
        "WebSearch" => str_field("query"),
        // `select:a,b` loads named tools; anything else is a keyword search.
        "ToolSearch" => str_field("query").map(|q| match q.strip_prefix("select:") {
            Some(names) => names
                .split(',')
                .map(str::trim)
                .collect::<Vec<_>>()
                .join(", "),
            None => format!("\"{q}\""),
        }),
        "Task" | "Agent" => str_field("description").or_else(|| str_field("prompt")),
        "TaskOutput" | "TaskStop" => str_field("task_id"),
        "AskUserQuestion" => v
            .get("questions")
            .and_then(Value::as_array)
            .and_then(|qs| qs.first())
            .and_then(|q| q.get("question"))
            .and_then(Value::as_str)
            .map(str::to_string),
        "TodoWrite" => v
            .get("todos")
            .and_then(Value::as_array)
            .map(|t| format!("{} todos", t.len())),
        _ => None,
    }
}

/// Last path segment, prefixed with its parent when that is informative
/// (`…/Test/Deadlock.c` for deep asset trees).
fn short_path(path: String) -> String {
    let mut parts: Vec<&str> = path.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    match parts.len() {
        0 => path,
        1 => parts.remove(0).to_string(),
        _ => {
            let file = parts.pop().unwrap_or_default();
            let parent = parts.last().unwrap_or(&"");
            format!("{parent}/{file}")
        }
    }
}

/// Expanded output: TaskOutput gets its status view, diffs render as diffs,
/// anything else is the text with model-only wrappers and input echoes removed.
fn output_view(tool: &str, display: Option<&Value>, output_text: &str) -> AnyElement {
    if tool == "TaskOutput" && (display.is_some() || !output_text.is_empty()) {
        return task_output_view(display, output_text);
    }
    let text = displayed_result(canonical_tool(tool), clean_tool_output(output_text));
    if text.is_empty() {
        return div().into_any_element();
    }
    if crate::shared::diff_view::looks_like_diff(&text) {
        return crate::shared::diff_view::render_diff(&text);
    }
    div()
        .w_full()
        .min_w_0()
        .text_color(rgb(MUTED))
        .child(mono("output", &text))
        .into_any_element()
}

/// Expanded input view: Bash shows the command, edits show a synthesized
/// +/- diff, other tools get a readable per-tool or key/value view.
fn input_view(label: &str, input: Option<&Value>, raw: &str, id_base: u64) -> AnyElement {
    if let Some(v) = input {
        match label {
            "Bash" => {
                if let Some(cmd) = v.get("command").and_then(Value::as_str) {
                    return mono_block("command", cmd);
                }
            }
            "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
                if let Some(diff) = edit_diff(v, label) {
                    return div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .font_family(UI_FONT)
                                .text_size(px(ui_size(11.5)))
                                .text_color(rgb(MUTED))
                                .child(selectable(
                                    "file-path",
                                    v.get("file_path")
                                        .and_then(Value::as_str)
                                        .or_else(|| v.get("notebook_path").and_then(Value::as_str))
                                        .unwrap_or("")
                                        .to_string(),
                                )),
                        )
                        .child(crate::shared::diff_view::render_diff(&diff))
                        .into_any_element();
                }
            }
            _ => {}
        }
    }
    generic_input_view(label, input, raw, id_base)
}

fn mono_block(slot: &'static str, text: &str) -> AnyElement {
    div()
        .font_family(MONO_FONT)
        .text_size(px(ui_size(11.)))
        .text_color(rgb(TEXT))
        .child(selectable(slot, text.to_string()))
        .into_any_element()
}

/// `-` old lines then `+` new lines — `render_diff` colors by line prefix.
fn edit_diff(v: &Value, label: &str) -> Option<String> {
    let pair = |old: &Value, new: &Value| -> Option<String> {
        let old = old.as_str()?;
        let new = new.as_str()?;
        let mut out = String::new();
        for line in old.lines() {
            out.push('-');
            out.push_str(line);
            out.push('\n');
        }
        for line in new.lines() {
            out.push('+');
            out.push_str(line);
            out.push('\n');
        }
        Some(out)
    };
    if label == "MultiEdit" {
        let edits = v.get("edits").and_then(Value::as_array)?;
        let mut out = String::new();
        for e in edits {
            // Skip partial entries instead of aborting the whole edit.
            if let (Some(old), Some(new)) = (e.get("old_string"), e.get("new_string"))
                && let Some(d) = pair(old, new)
            {
                out.push_str(&d);
            }
        }
        (!out.is_empty()).then_some(out)
    } else if label == "Write" {
        let content = v.get("content").and_then(Value::as_str)?;
        let mut out = String::new();
        for line in content.lines() {
            out.push('+');
            out.push_str(line);
            out.push('\n');
        }
        (!out.is_empty()).then_some(out)
    } else {
        pair(v.get("old_string")?, v.get("new_string")?)
    }
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod selection_tests;
