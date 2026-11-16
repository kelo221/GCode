//! Expanded tool-card bodies for tools without a diff/command view:
//! AskUserQuestion, ExitPlanMode, TodoWrite, Agent/Task, TaskOutput, and a
//! readable key/value view for everything else. Raw wire text is only shown
//! when the input is not (yet) valid JSON, e.g. while it is still streaming.

use crate::shared::markdown::render_markdown;
use crate::shared::selectable::SelectableText;
use crate::shared::theme::{MONO_FONT, MUTED, SUCCESS, TAG, TEXT, ui_size};
use crate::shared::theme_colors::color as rgb;
use crate::transcript::tagged_text::parse_task_output;
use gpui::{AnyElement, Div, FontWeight, SharedString, div, prelude::*, px};
use serde_json::Value;

/// MCP-namespaced names (`mcp__server__Tool`) dispatch on their last segment.
pub(crate) fn canonical_tool(name: &str) -> &str {
    name.rsplit("__").next().unwrap_or(name)
}

fn column() -> Div {
    div().w_full().min_w_0().flex().flex_col().gap_1()
}

fn muted(text: impl Into<String>) -> Div {
    div()
        .text_size(px(ui_size(11.5)))
        .text_color(rgb(MUTED))
        .child(text.into())
}

/// Selectable text; `slot` must be unique within the tool body.
pub(crate) fn selectable(
    slot: impl Into<SharedString>,
    text: impl Into<SharedString>,
) -> SelectableText {
    SelectableText::new(slot.into(), text)
}

fn body(slot: impl Into<SharedString>, text: impl Into<String>) -> Div {
    let slot = slot.into();
    #[cfg(test)]
    let target = format!("selectable-{slot}");
    let el = div()
        .w_full()
        .text_size(px(ui_size(13.)))
        .text_color(rgb(TEXT))
        .child(selectable(slot, text.into()));
    #[cfg(test)]
    let el = crate::app::test_support::track_children(el, vec![target]);
    el
}

pub(crate) fn mono(slot: impl Into<SharedString>, text: &str) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .font_family(MONO_FONT)
        .text_size(px(ui_size(11.5)))
        .text_color(rgb(TEXT))
        // gpui does not draw tabs; expand them so `cat -n` columns stay apart.
        .child(selectable(slot, text.replace('\t', "    ")))
        .into_any_element()
}

fn badge(text: &str) -> Div {
    div()
        .flex_none()
        .px_1p5()
        .rounded_sm()
        .bg(rgb(TAG))
        .text_size(px(ui_size(11.)))
        .text_color(rgb(TEXT))
        .child(text.to_string())
}

fn str_of<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn ask_user_question(v: &Value) -> Option<AnyElement> {
    let questions = v.get("questions")?.as_array()?;
    let mut col = column().gap_2();
    for (i, q) in questions.iter().enumerate() {
        let options: Vec<&str> = q
            .get("options")
            .and_then(Value::as_array)
            .map(|os| os.iter().filter_map(|o| str_of(o, "label")).collect())
            .unwrap_or_default();
        col = col.child(
            column()
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .items_center()
                        .children(str_of(q, "header").map(badge))
                        .when(q.get("multiSelect") == Some(&Value::Bool(true)), |el| {
                            el.child(muted("multi-select"))
                        }),
                )
                .child(body(
                    format!("question-{i}"),
                    str_of(q, "question").unwrap_or_default(),
                ))
                .when(!options.is_empty(), |el| {
                    el.child(muted(format!("Options: {}", options.join(" · "))))
                }),
        );
    }
    Some(col.into_any_element())
}

fn todo_list(v: &Value) -> Option<AnyElement> {
    let todos = v.get("todos")?.as_array()?;
    let mut col = column();
    for (i, t) in todos.iter().enumerate() {
        let status = str_of(t, "status").unwrap_or("pending");
        let (mark, color) = match status {
            "completed" => ("☑", SUCCESS),
            "in_progress" => ("◐", TEXT),
            _ => ("☐", MUTED),
        };
        col = col.child(
            div()
                .flex()
                .gap_2()
                .w_full()
                .min_w_0()
                .text_size(px(ui_size(13.)))
                .child(div().flex_none().text_color(rgb(color)).child(mark))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(rgb(if status == "completed" { MUTED } else { TEXT }))
                        .when(status == "completed", |el| el.line_through())
                        .child(selectable(
                            format!("todo-{i}"),
                            str_of(t, "content").unwrap_or_default().to_string(),
                        )),
                ),
        );
    }
    Some(col.into_any_element())
}

fn agent_task(v: &Value, id_base: u64) -> Option<AnyElement> {
    let prompt = str_of(v, "prompt")?;
    Some(
        column()
            .gap_2()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .children(str_of(v, "subagent_type").map(badge))
                    .children(str_of(v, "description").map(|d| {
                        body("description", d.to_string()).font_weight(FontWeight::MEDIUM)
                    })),
            )
            .child(render_markdown(prompt, id_base, false))
            .into_any_element(),
    )
}

/// Generic readable view: one muted key over its value per top-level field.
fn key_values(v: &Value) -> Option<AnyElement> {
    let obj = v.as_object()?;
    let mut col = column().gap_2();
    for (k, val) in obj {
        let slot = format!("field-{k}");
        let value: AnyElement = match val {
            Value::String(s) => body(slot, s.clone()).into_any_element(),
            Value::Null => continue,
            Value::Bool(_) | Value::Number(_) => body(slot, val.to_string()).into_any_element(),
            _ => mono(slot, &serde_json::to_string_pretty(val).unwrap_or_default()),
        };
        col = col.child(column().gap_0p5().child(muted(k.clone())).child(value));
    }
    Some(col.into_any_element())
}

/// Input view for tools without a bespoke command/diff renderer.
pub(crate) fn generic_input_view(
    tool: &str,
    input: Option<&Value>,
    raw: &str,
    id_base: u64,
) -> AnyElement {
    let Some(v) = input else {
        return mono("raw-input", raw);
    };
    let view = match canonical_tool(tool) {
        "AskUserQuestion" => ask_user_question(v),
        "ExitPlanMode" => str_of(v, "plan").map(|p| render_markdown(p, id_base, false)),
        "TodoWrite" => todo_list(v),
        "Agent" | "Task" => agent_task(v, id_base),
        _ => None,
    };
    view.or_else(|| key_values(v))
        .unwrap_or_else(|| mono("raw-input", raw))
}

/// TaskOutput body: status line plus output, from the row's `task_output`
/// display when present, else from the tagged result text.
pub(crate) fn task_output_view(display: Option<&Value>, output_text: &str) -> AnyElement {
    let from_display = display.filter(|d| str_of(d, "kind") == Some("task_output"));
    let (retrieval, status, output, truncated) = match from_display {
        Some(d) => (
            str_of(d, "retrievalStatus").map(str::to_string),
            str_of(d, "taskStatus").map(str::to_string),
            str_of(d, "output").map(str::to_string),
            d.get("truncated") == Some(&Value::Bool(true)),
        ),
        None => match parse_task_output(output_text) {
            Some(t) => (Some(t.retrieval_status), t.status, t.output, false),
            None => (None, None, Some(output_text.to_string()), false),
        },
    };
    let state = match (retrieval.as_deref(), status.as_deref()) {
        (Some("not_ready"), _) => "Still running".to_string(),
        (Some("timeout"), _) => "Timed out waiting for output".to_string(),
        (_, Some(s)) => format!("Task {s}"),
        (Some(r), None) => r.to_string(),
        (None, None) => String::new(),
    };
    column()
        .gap_2()
        .when(!state.is_empty(), |el| el.child(muted(state)))
        .children(
            output
                .filter(|o| !o.trim().is_empty())
                .map(|o| mono("task-output", &o)),
        )
        .when(truncated, |el| el.child(muted("Output truncated")))
        .into_any_element()
}

/// `-` old lines then `+` new lines — `render_diff` colors by line prefix.
pub(crate) fn edit_diff(v: &Value, label: &str) -> Option<String> {
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
mod tests {
    use super::canonical_tool;

    #[test]
    fn mcp_names_dispatch_on_last_segment() {
        assert_eq!(
            canonical_tool("mcp__srv__AskUserQuestion"),
            "AskUserQuestion"
        );
        assert_eq!(canonical_tool("TodoWrite"), "TodoWrite");
    }
}
