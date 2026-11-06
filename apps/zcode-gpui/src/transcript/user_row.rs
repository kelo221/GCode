//! User-input rows: the right-aligned message bubble (Desktop
//! `ConversationUserInputBubble`) with its send time, and background results
//! (`<task-notification>` text) as a compact left-aligned notice instead of
//! raw XML.

use crate::composer::attachment::AttachmentRef;
use crate::shared::local_time::format_timestamp;
use crate::shared::theme::{BORDER, MUTED, SUBTLEST, SURFACE, TEXT, ui_size};
use crate::shared::theme_colors::color as rgb;
use crate::transcript::tagged_text::{parse_task_notification, strip_system_reminders};
use crate::transcript::tool_images::ImageRef;
use gpui::{AnyElement, Context, Div, FontWeight, div, prelude::*, px};

const USER_GROUP: &str = "user-message";

pub(crate) struct UserRowView<'a> {
    pub row_id: u64,
    pub entity_id: &'a str,
    pub text: &'a str,
    pub can_edit: bool,
    pub origin: &'a str,
    pub created_at: Option<u64>,
    pub attachments: &'a [AttachmentRef],
}

/// Bubble text with task / skill / file mentions shown as pills (Desktop
/// `ConversationUserInputContent`); Copy and Edit still get the raw markdown.
fn mention_text(row_id: u64, raw: &str) -> crate::shared::selectable::SelectableText {
    let (shown, pills) = crate::shared::mention_parse::display_with_pills(raw);
    let style = mention_pill_style();
    crate::shared::selectable::SelectableText::new(
        gpui::ElementId::NamedInteger("user-text".into(), row_id),
        shown,
    )
    .styles(pills.into_iter().map(|r| (r, style)).collect())
}

pub(crate) fn mention_pill_style() -> gpui::HighlightStyle {
    let mut bg = rgb(crate::shared::theme::ACCENT);
    bg.a = 0.14;
    gpui::HighlightStyle {
        color: Some(rgb(crate::shared::theme::ACCENT).into()),
        background_color: Some(bg.into()),
        font_weight: Some(FontWeight::MEDIUM),
        ..Default::default()
    }
}

fn is_image(a: &AttachmentRef) -> bool {
    a.mime.to_ascii_lowercase().starts_with("image/")
}

/// Image attachments of a sent message as thumbnails; the bytes are read back
/// through the CLI's `v4/attachment/read` (`previewRef` first, like Electron).
pub(crate) fn attachment_images(session: &str, attachments: &[AttachmentRef]) -> Vec<ImageRef> {
    attachments
        .iter()
        .filter(|a| is_image(a))
        .map(|a| ImageRef::Attachment {
            session: session.to_string(),
            reference: a.preview_ref.clone().unwrap_or_else(|| a.reference.clone()),
            mime: a.mime.clone(),
        })
        .collect()
}

/// Chips for the other attachments, and for images when there is no session
/// to read them from.
fn attachment_chips(attachments: &[&AttachmentRef]) -> Option<Div> {
    (!attachments.is_empty()).then(|| {
        div()
            .w_full()
            .flex()
            .flex_wrap()
            .justify_end()
            .gap_1()
            .max_w(px(576.))
            .children(attachments.iter().map(|a| {
                let kind = if is_image(a) { "Image" } else { "File" };
                div()
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(BORDER))
                    .text_size(px(ui_size(12.)))
                    .text_color(rgb(MUTED))
                    .child(format!(
                        "{kind}: {} · {}",
                        a.file_name,
                        crate::composer::attachment::format_bytes(a.bytes)
                    ))
            }))
    })
}

pub(crate) fn timestamp_label(created_at: Option<u64>) -> Option<Div> {
    created_at.map(|ms| {
        div()
            .flex_none()
            .text_size(px(ui_size(11.)))
            .text_color(rgb(SUBTLEST))
            .child(format_timestamp(ms))
    })
}

fn notification_card(text: &str, created_at: Option<u64>, row_id: u64) -> Option<AnyElement> {
    let n = parse_task_notification(text)?;
    let title = match (&n.description, n.task_id.is_empty()) {
        (Some(d), _) => d.clone(),
        (None, false) => n.task_id.clone(),
        (None, true) => "Background task".to_string(),
    };
    let body = n.body();
    Some(
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(ui_size(12.)))
                    .text_color(rgb(MUTED))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!("Background result · {}", n.status)),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(title))
                    .children(timestamp_label(created_at)),
            )
            .when(!body.is_empty(), |el| {
                el.child(crate::shared::markdown::render_passive_markdown(
                    &body, row_id,
                ))
            })
            .into_any_element(),
    )
}

impl crate::app::root::RootView {
    pub(crate) fn render_user_row(
        &self,
        v: UserRowView<'_>,
        row_actions: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let looks_like_notice = v.origin == "backgroundResult"
            || v.text.trim_start().starts_with("<task-notification>");
        if looks_like_notice && let Some(card) = notification_card(v.text, v.created_at, v.row_id) {
            return Some(card);
        }
        let shown = strip_system_reminders(v.text);
        if shown.is_empty() && !v.text.is_empty() {
            return None;
        }
        let session = self.state.read(cx).active.clone();
        let images = session
            .as_deref()
            .map(|s| attachment_images(s, v.attachments))
            .unwrap_or_default();
        let chips: Vec<&AttachmentRef> = v
            .attachments
            .iter()
            .filter(|a| session.is_none() || !is_image(a))
            .collect();
        // 修复原因：外层按内容收缩宽度时，换行的缩略图行高度只按一行计算，第二张图会
        // 压住时间戳和下一条消息。w_full 让换行在确定宽度下布局，高度随行数增长。
        let thumbnails = self.images_row(v.row_id, images, true, cx).map(|row| {
            div()
                .w_full()
                .max_w(px(576.))
                .min_w_0()
                .flex()
                .justify_end()
                .child(row)
        });
        #[cfg(test)]
        let tracked: Vec<String> = [
            Some("bubble"),
            thumbnails.is_some().then_some("thumbs"),
            (!chips.is_empty()).then_some("chips"),
            Some("footer"),
        ]
        .into_iter()
        .flatten()
        .map(|part| format!("user-{part}-{}", v.row_id))
        .collect();
        let column = div();
        #[cfg(test)]
        let column = crate::app::test_support::track_children(column, tracked);
        Some(
            column
                .group(USER_GROUP)
                .w_full()
                .flex()
                .flex_col()
                .items_end()
                .gap_1()
                .child(
                    div()
                        .max_w(px(576.))
                        .min_w_0()
                        .bg(rgb(SURFACE))
                        .border_1()
                        .border_color(rgb(BORDER))
                        .rounded_tl(px(12.))
                        .rounded_bl(px(12.))
                        .rounded_br(px(12.))
                        .rounded_tr(px(2.))
                        .px_4()
                        .py_3()
                        .text_size(px(ui_size(14.)))
                        .text_color(rgb(TEXT))
                        .child(mention_text(v.row_id, &shown)),
                )
                .children(thumbnails)
                .children(attachment_chips(&chips))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            self.user_message_actions(
                                v.row_id,
                                v.entity_id.to_string(),
                                shown,
                                v.can_edit && row_actions,
                                cx,
                            )
                            .opacity(0.)
                            .group_hover(USER_GROUP, |s| s.opacity(1.)),
                        )
                        .children(timestamp_label(v.created_at)),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(reference: &str, mime: &str, preview: Option<&str>) -> AttachmentRef {
        AttachmentRef {
            reference: reference.into(),
            file_name: "f".into(),
            mime: mime.into(),
            bytes: 1,
            preview_ref: preview.map(Into::into),
        }
    }

    #[test]
    fn only_image_attachments_become_thumbnails_read_by_preview_ref_first() {
        let list = [
            attachment("zcode-artifact://a/1", "image/png", None),
            attachment("C:/tmp/notes.txt", "text/plain", None),
            attachment(
                "zcode-artifact://a/2",
                "IMAGE/JPEG",
                Some("zcode-artifact://a/2p"),
            ),
        ];
        let refs = attachment_images("s1", &list);
        assert_eq!(
            refs,
            vec![
                ImageRef::Attachment {
                    session: "s1".into(),
                    reference: "zcode-artifact://a/1".into(),
                    mime: "image/png".into(),
                },
                ImageRef::Attachment {
                    session: "s1".into(),
                    reference: "zcode-artifact://a/2p".into(),
                    mime: "IMAGE/JPEG".into(),
                },
            ]
        );
    }
}
