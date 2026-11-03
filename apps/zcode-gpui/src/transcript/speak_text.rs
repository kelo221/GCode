//! The sentence a `Speak` call said aloud, shown in the transcript like agent
//! reply text (VOICE.md "The spoken sentence is also shown as text").

use crate::shared::theme::{MUTED, TEXT, icon, ui_size};
use crate::shared::theme_colors::color as rgb;
use crate::transcript::tool_inputs::canonical_tool;
use ely_gpui_component::primitives::IconName;
use gpui::{AnyElement, IntoElement, ParentElement, Styled, div, px};
use serde_json::Value;

/// The `message` of a Speak call once its input parses; `None` for other
/// tools, partial (still streaming) input, or an empty message.
pub(crate) fn spoken_message(tool_name: &str, input_text: &str) -> Option<String> {
    if canonical_tool(tool_name) != "Speak" {
        return None;
    }
    let input: Value = serde_json::from_str(input_text).ok()?;
    let message = input.get("message")?.as_str()?.trim();
    (!message.is_empty()).then(|| message.to_owned())
}

pub(crate) fn spoken_message_view(message: &str, row_id: u64) -> AnyElement {
    div()
        .w_full()
        .flex()
        .items_start()
        .gap_2()
        .text_size(px(ui_size(14.)))
        .text_color(rgb(TEXT))
        .child(
            div()
                .flex_none()
                .pt_0p5()
                .child(icon(IconName::Volume2, 16., MUTED)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(crate::shared::markdown::render_markdown(
                    message, row_id, false,
                )),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::spoken_message;

    #[test]
    fn only_a_parsed_speak_message_is_mirrored() {
        let input = r#"{"message":"  Task is done. ","kind":"done"}"#;
        assert_eq!(
            spoken_message("Speak", input).as_deref(),
            Some("Task is done.")
        );
        assert_eq!(
            spoken_message("mcp__x__Speak", input).as_deref(),
            Some("Task is done.")
        );
        // 输入仍在流式传输时 JSON 不完整，不显示半句话。
        assert_eq!(spoken_message("Speak", r#"{"message":"Task is"#), None);
        assert_eq!(spoken_message("Speak", r#"{"message":"  "}"#), None);
        assert_eq!(spoken_message("Bash", input), None);
    }
}
