//! Transcript row rendering for the chat surface: turns, user inputs,
//! assistant text with streaming cursor, collapsible reasoning, and expandable tool cards.

use crate::conversation::model::Row;
use crate::shared::theme::TEXT;
use crate::shared::theme::ui_size;
use crate::shared::theme_colors::color as rgb;
use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};

impl crate::app::root::RootView {
    /// `row_actions` is false until the snapshot's log epoch is known; row
    /// commands cannot be sent without `baseLogEpoch`.
    pub(crate) fn transcript_row(
        &self,
        row: &Row,
        row_actions: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match row {
            Row::TurnHeader {
                row_id,
                entity_id,
                state,
                active_ms,
                file_changes,
                can_rewind,
            } => Some(crate::conversation::turn_meta::render_turn_header(
                (*row_id, entity_id),
                state,
                *active_ms,
                file_changes.as_ref(),
                *can_rewind && row_actions,
                self.confirm.as_deref() == Some(format!("undo:{row_id}").as_str()),
                cx,
            )),
            Row::UserInput {
                row_id,
                entity_id,
                text,
                can_edit,
                origin,
                created_at,
                attachments,
            } => self.render_user_row(
                crate::transcript::user_row::UserRowView {
                    row_id: *row_id,
                    entity_id,
                    text,
                    can_edit: *can_edit,
                    origin,
                    created_at: *created_at,
                    attachments,
                },
                row_actions,
                cx,
            ),
            Row::AssistantText {
                row_id,
                entity_id,
                text,
                state,
                can_retry,
                created_at,
            } => {
                let streaming = state == "streaming";
                crate::shared::markdown::file_exists::set_root(self.active_workspace_path(cx));
                let can_retry = *can_retry && !streaming && row_actions;
                let text = crate::transcript::tagged_text::remaining_items_as_markdown(text);
                let text = text.as_ref();
                let images = if streaming || !text.contains("![") {
                    None
                } else {
                    let refs = crate::shared::markdown::image_targets(text)
                        .into_iter()
                        .map(crate::transcript::tool_images::ImageRef::File)
                        .collect();
                    self.tool_images_view(*row_id, refs, cx)
                };
                Some(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap_1()
                        .text_size(px(ui_size(14.)))
                        .text_color(rgb(TEXT))
                        .child(crate::shared::markdown::render_markdown(
                            text, *row_id, streaming,
                        ))
                        .children(images)
                        .when(!streaming && !text.is_empty(), |el| {
                            el.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(self.assistant_message_actions(
                                        *row_id,
                                        entity_id.clone(),
                                        text.to_string(),
                                        can_retry,
                                        cx,
                                    ))
                                    .children(crate::transcript::user_row::timestamp_label(
                                        *created_at,
                                    )),
                            )
                        })
                        .into_any_element(),
                )
            }
            Row::Reasoning {
                row_id,
                text,
                duration_ms,
            } => crate::transcript::reasoning::has_reasoning_text(text)
                .then(|| self.render_reasoning(*row_id, text, *duration_ms, cx)),
            Row::ToolCall {
                row_id,
                label,
                status,
                input_text,
                output_text,
                tool_call_id,
                tool_name,
                display,
            } => {
                let maybe_subagent = tool_call_id
                    .as_deref()
                    .and_then(|tcid| self.find_subagent_for_tool_call(tcid, cx));
                let tool_el = self.render_tool_row(
                    crate::transcript::tool_card::ToolCallView {
                        row_id: *row_id,
                        label,
                        tool_name,
                        status,
                        input_text,
                        output_text,
                        display: display.as_ref(),
                    },
                    cx,
                );
                // Speak 的句子以前只藏在折叠的工具行里；按回复文本显示在工具行上方。
                if let Some(message) =
                    crate::transcript::speak_text::spoken_message(tool_name, input_text)
                {
                    return Some(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(crate::transcript::speak_text::spoken_message_view(
                                &message, *row_id,
                            ))
                            .child(tool_el)
                            .into_any_element(),
                    );
                }
                if let Some(subagent_row) = maybe_subagent {
                    let subagent_el = self.render_subagent_card(&subagent_row, true, cx);
                    Some(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .child(tool_el)
                            .child(subagent_el)
                            .into_any_element(),
                    )
                } else {
                    Some(tool_el)
                }
            }
            Row::Subagent {
                parent_tool_call_id,
                ..
            } => {
                let is_paired = parent_tool_call_id
                    .as_deref()
                    .is_some_and(|ptid| self.has_tool_call_with_id(ptid, cx));
                if is_paired {
                    None
                } else {
                    Some(self.render_subagent_card(row, false, cx))
                }
            }
            Row::Other => None,
        }
    }
}
