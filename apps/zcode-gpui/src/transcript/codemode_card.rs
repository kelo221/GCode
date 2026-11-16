//! `codemode` tool rows (apps/zcode-cli/docs/design/v2/tool/21-codemode.md): the
//! script as highlighted JavaScript, and the nested tool calls it made, read from
//! the result's `{kind:"codemode"}` display. Split from tool_card.rs for the 400-line cap.
use crate::shared::theme::{DANGER, MUTED, SUBTLEST, UI_FONT, ui_size};
use crate::shared::theme_colors::color as rgb;
use gpui::{AnyElement, IntoElement, ParentElement, Styled, div, px};
use serde_json::Value;

pub(crate) const CODEMODE_TOOL: &str = "codemode";

/// First line of the script that is not blank or a comment (`// @options:` included).
pub(crate) fn script_summary(input: &Value) -> Option<String> {
    let code = input.get("code")?.as_str()?;
    let mut in_block = false;
    code.lines().map(str::trim).find_map(|line| {
        if in_block {
            in_block = !line.contains("*/");
            return None;
        }
        if line.starts_with("/*") {
            in_block = !line.contains("*/");
            return None;
        }
        (!line.is_empty() && !line.starts_with("//")).then(|| line.to_string())
    })
}

struct NestedCall<'a> {
    name: &'a str,
    status: &'a str,
    duration_ms: u64,
}

fn nested_calls(display: Option<&Value>) -> Option<(Vec<NestedCall<'_>>, bool)> {
    let display = display?;
    if display.get("kind").and_then(Value::as_str) != Some(CODEMODE_TOOL) {
        return None;
    }
    let calls = display
        .get("calls")?
        .as_array()?
        .iter()
        .filter_map(|c| {
            Some(NestedCall {
                name: c.get("name")?.as_str()?,
                status: c.get("status")?.as_str()?,
                duration_ms: c.get("durationMs").and_then(Value::as_u64).unwrap_or(0),
            })
        })
        .collect();
    let truncated = display.get("truncated").and_then(Value::as_bool) == Some(true);
    Some((calls, truncated))
}

/// "· 3 tool calls" after the summary of a finished script.
pub(crate) fn calls_chip(display: Option<&Value>) -> Option<AnyElement> {
    let (calls, truncated) = nested_calls(display)?;
    if calls.is_empty() {
        return None;
    }
    let count = format!("{}{}", calls.len(), if truncated { "+" } else { "" });
    let (english, chinese) = (
        format!("· {count} tool calls"),
        format!("· {count} 次工具调用"),
    );
    let text = crate::shared::i18n::label(&english, &chinese).to_string();
    Some(
        div()
            .flex_none()
            .font_family(UI_FONT)
            .text_color(rgb(SUBTLEST))
            .child(text)
            .into_any_element(),
    )
}

/// The script, highlighted as JavaScript.
pub(crate) fn script_view(input: &Value, id_base: u64) -> Option<AnyElement> {
    let code = input.get("code")?.as_str()?;
    Some(crate::shared::markdown::code::code_block(
        id_base, code, "js", None,
    ))
}

const CONSOLE_OPEN: &str = "<console_output>
";
const CONSOLE_CLOSE: &str = "
</console_output>";

/// Splits the result text into the script output and its console lines. The
/// `<console_output>` wrapper is for the model; the card shows a label instead.
pub(crate) fn split_console(text: &str) -> (String, Option<&str>) {
    let Some(start) = text.find(CONSOLE_OPEN) else {
        return (text.to_string(), None);
    };
    let body_start = start + CONSOLE_OPEN.len();
    let Some(len) = text[body_start..].find(CONSOLE_CLOSE) else {
        return (text.to_string(), None);
    };
    let console = &text[body_start..body_start + len];
    let rest = format!(
        "{}{}",
        text[..start].trim_end(),
        &text[body_start + len + CONSOLE_CLOSE.len()..]
    );
    (rest.trim_end().to_string(), Some(console))
}

/// Result text with the console lines under a muted "Console" label.
pub(crate) fn output_view(text: &str) -> AnyElement {
    let (main, console) = split_console(text);
    let mono = crate::transcript::tool_inputs::mono;
    div()
        .flex()
        .flex_col()
        .gap_1()
        .w_full()
        .min_w_0()
        .text_color(rgb(MUTED))
        .child(mono("output", &main))
        .children(console.map(|lines| {
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .pt_1()
                .child(
                    div()
                        .font_family(UI_FONT)
                        .text_size(px(ui_size(11.5)))
                        .text_color(rgb(SUBTLEST))
                        .child(crate::shared::i18n::label("Console", "控制台").to_string()),
                )
                .child(mono("console", lines))
        }))
        .into_any_element()
}

/// One line per nested call: name, status, duration.
pub(crate) fn calls_view(display: Option<&Value>) -> Option<AnyElement> {
    let (calls, truncated) = nested_calls(display)?;
    if calls.is_empty() {
        return None;
    }
    let rows = calls.into_iter().map(|call| {
        let failed = call.status != "ok";
        div()
            .flex()
            .gap_2()
            .min_w_0()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(rgb(MUTED))
                    .child(call.name.to_string()),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(rgb(if failed { DANGER } else { SUBTLEST }))
                    .child(match call.status {
                        "ok" => format!("{} ms", call.duration_ms),
                        other => other.to_string(),
                    }),
            )
    });
    Some(
        div()
            .flex()
            .flex_col()
            .gap_0p5()
            .font_family(UI_FONT)
            .text_size(px(ui_size(12.)))
            .children(rows)
            .children(truncated.then(|| {
                div()
                    .text_color(rgb(SUBTLEST))
                    .child(crate::shared::i18n::label(
                        "More calls not listed",
                        "更多调用未列出",
                    ))
            }))
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summary_skips_options_and_comments() {
        let input = json!({"code": "// @options: {\"timeout_ms\": 5}\n/* setup\n more */\n\nconst a = await tools.Read({file_path: 'x'});\nreturn a;"});
        assert_eq!(
            script_summary(&input).as_deref(),
            Some("const a = await tools.Read({file_path: 'x'});")
        );
        assert_eq!(script_summary(&json!({"code": "// only"})), None);
    }

    #[test]
    fn console_lines_are_split_from_the_result() {
        let text = "Script completed in 0.03s

a
b

<console_output>
hi
there
</console_output>

Script error: Error: boom";
        let (main, console) = split_console(text);
        assert_eq!(
            main,
            "Script completed in 0.03s

a
b

Script error: Error: boom"
        );
        assert_eq!(
            console,
            Some(
                "hi
there"
            )
        );
        assert_eq!(split_console("plain"), ("plain".to_string(), None));
        assert_eq!(
            split_console(
                "x
<console_output>
unterminated"
            )
            .1,
            None,
            "an unterminated block stays as text"
        );
    }

    #[test]
    fn calls_come_only_from_a_codemode_display() {
        let display = json!({"kind": "codemode", "ok": true, "truncated": true, "calls": [
            {"name": "Read", "status": "ok", "durationMs": 12},
            {"name": "Bash", "status": "error", "durationMs": 3}
        ]});
        let (calls, truncated) = nested_calls(Some(&display)).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].status, "error");
        assert!(truncated);
        assert!(nested_calls(Some(&json!({"kind": "file_diff"}))).is_none());
    }
}
