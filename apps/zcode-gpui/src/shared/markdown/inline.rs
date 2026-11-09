//! Inline markdown → gpui `StyledText` runs. Inline content must render as
//! ONE flowing text element (like the desktop app); a div per span renders
//! block-level and breaks every span onto its own line. Links stay clickable
//! via `InteractiveText` range callbacks.
//!
//! Spec: PARITY.md M2 "Streaming Markdown renderer".

use gpui::{
    AnyElement, Font, FontFeatures, FontStyle, FontWeight, Hsla, IntoElement, StrikethroughStyle,
    TextRun, UnderlineStyle, px,
};
use pulldown_cmark::{Event, Tag, TagEnd};
use std::ops::Range;

const BODY_FONT: &str = crate::shared::theme::UI_FONT;
const CODE_FONT: &str = crate::shared::theme::MONO_FONT;

fn c(hex: u32) -> Hsla {
    crate::shared::theme_colors::color(hex).into()
}

/// Flattened inline content: one string + one style run per formatting span
/// (runs concatenate to exactly `text`) + clickable link ranges.
#[derive(Default)]
pub(crate) struct Inline {
    pub text: String,
    pub runs: Vec<TextRun>,
    /// (byte range into `text`, url) — order matches click listener indices.
    pub links: Vec<(Range<usize>, String)>,
}

/// Character formatting accumulated across nested containers.
#[derive(Clone)]
pub(crate) struct Style {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) strike: bool,
    pub(crate) code: bool,
    pub(crate) link: bool,
}

impl Style {
    pub(crate) fn default() -> Self {
        Self {
            bold: false,
            italic: false,
            strike: false,
            code: false,
            link: false,
        }
    }
}

fn font_of(st: &Style) -> Font {
    Font {
        family: if st.code {
            CODE_FONT.into()
        } else {
            BODY_FONT.into()
        },
        features: FontFeatures::default(),
        fallbacks: None,
        weight: if st.bold {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        },
        style: if st.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
    }
}

fn color_of(st: &Style) -> Hsla {
    if st.link {
        c(crate::shared::theme::LINK)
    } else if st.strike {
        c(crate::shared::theme::MUTED)
    } else {
        c(crate::shared::theme::TEXT)
    }
}

/// Append `len` bytes styled by `st`, merging with the previous run when the
/// formatting is identical (fewer runs = faster shaping).
fn push_run(inline: &mut Inline, len: usize, st: &Style) {
    if len == 0 {
        return;
    }
    let font = font_of(st);
    let color = color_of(st);
    // Desktop inline code: foreground text on the tag color at half alpha.
    let background = st
        .code
        .then(|| crate::shared::theme_colors::tinted(crate::shared::theme::TAG, 0.5).into());
    let underline = st.link.then(|| UnderlineStyle {
        thickness: px(1.),
        color: None,
        wavy: false,
    });
    let strike = st.strike.then(|| StrikethroughStyle {
        thickness: px(1.),
        color: None,
    });
    // Decorations must match too, or a strikethrough/underline would spread
    // onto (or vanish from) an adjacent span with the same font and color.
    if let Some(last) = inline.runs.last_mut()
        && last.font == font
        && last.color == color
        && last.background_color == background
        && last.underline == underline
        && last.strikethrough == strike
    {
        last.len += len;
        return;
    }
    inline.runs.push(TextRun {
        len,
        font,
        color,
        background_color: background,
        underline,
        strikethrough: strike,
    });
}

/// Consume inline events from `events[*pos..]` into one `Inline`. Stops at
/// (and consumes) the matching `stop` end tag, or — with `stop = None` — at
/// the first block-level `End` (tight-list content), which it leaves for the
/// block walker. Inline-level ends (e.g. `End(Strong)`) are always consumed:
/// stopping on those would truncate the item after its bold lead.
pub(crate) fn render_inlines(
    events: &[Event],
    pos: &mut usize,
    stop: Option<&TagEnd>,
    base: &Style,
) -> Inline {
    let mut inline = Inline::default();
    let st = base.clone();
    while *pos < events.len() {
        match &events[*pos] {
            Event::End(end) => match stop {
                Some(stop_tag) if end == stop_tag => {
                    *pos += 1;
                    return inline;
                }
                None if is_block_end(end) => return inline, // left for the block walker
                _ => *pos += 1,
            },
            // 紧凑列表项的文字后面可以直接跟嵌套列表（`1. 标题` 换行 `   - 子项`），
            // 中间没有 End(Item)。原先这里把 Start(List)/Start(Item) 当成未知事件跳过，
            // 子项文字被拼进父项，嵌套列表的 End(List) 又被外层列表误认成自己的结束，
            // 顶层遍历随之提前停止，后文全部丢失。块级 Start 必须留给块遍历器处理。
            Event::Start(tag) if stop.is_none() && is_block_start(tag) => return inline,
            Event::Rule if stop.is_none() => return inline,
            Event::Text(t) => {
                push_run(&mut inline, t.len(), &st);
                inline.text.push_str(t);
                *pos += 1;
            }
            Event::Code(cd) => {
                let mut code_st = st.clone();
                code_st.code = true;
                // Inside a Markdown link the link's own target wins.
                // 只有确实存在的文件才渲染成链接：`ExplosionUtils.Explode` 这类符号名
                // 形似“文件名.扩展名”，原先被当成链接，点击后只报 File not found。
                let file_ref = (!st.link)
                    .then(|| super::file_ref::code_file_ref(cd))
                    .flatten()
                    .filter(|r| super::file_exists::file_exists(&r.path) != Some(false));
                code_st.link |= file_ref.is_some();
                let start = inline.text.len();
                push_run(&mut inline, cd.len(), &code_st);
                inline.text.push_str(cd);
                if file_ref.is_some() {
                    inline.links.push((
                        start..inline.text.len(),
                        format!("{}{}", super::file_ref::FILE_LINK_SCHEME, cd.trim()),
                    ));
                }
                *pos += 1;
            }
            Event::SoftBreak | Event::HardBreak => {
                push_run(&mut inline, 1, &st);
                inline.text.push(' ');
                *pos += 1;
            }
            Event::Start(
                tag @ (Tag::Strong | Tag::Emphasis | Tag::Strikethrough | Tag::Link { .. }),
            ) => {
                let mut inner = st.clone();
                let mut url = None;
                match tag {
                    Tag::Strong => inner.bold = true,
                    Tag::Emphasis => inner.italic = true,
                    Tag::Strikethrough => inner.strike = true,
                    Tag::Link { dest_url, .. } => {
                        url = Some(match super::file_ref::link_file_target(dest_url) {
                            Some(path) => format!("{}{path}", super::file_ref::FILE_LINK_SCHEME),
                            None => dest_url.to_string(),
                        });
                        inner.link = true;
                    }
                    _ => {}
                }
                let end = tag.to_end();
                let start_byte = inline.text.len();
                *pos += 1;
                let inner_inline = render_inlines(events, pos, Some(&end), &inner);
                inline.text.push_str(&inner_inline.text);
                inline.runs.extend(inner_inline.runs);
                // Inner links (e.g. images inside the link text) are relative
                // to the inner text and shift by the splice offset; the link's
                // own range is already absolute.
                for (range, link) in inner_inline.links {
                    inline
                        .links
                        .push((range.start + start_byte..range.end + start_byte, link));
                }
                if let Some(url) = url {
                    inline.links.push((start_byte..inline.text.len(), url));
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                // v1: images render as a link to their target.
                // 本地图片路径原先把原始路径直接当 URL，点击无效；改走与普通文件链接相同的
                // `zcode-file:` 目标，点击后用系统默认程序打开。
                let mut img = st.clone();
                img.link = true;
                let label = format!("[image: {dest_url}]");
                push_run(&mut inline, label.len(), &img);
                inline.text.push_str(&label);
                let url = match super::file_ref::link_file_target(dest_url) {
                    Some(path) => format!("{}{path}", super::file_ref::FILE_LINK_SCHEME),
                    None => dest_url.to_string(),
                };
                inline
                    .links
                    .push((inline.text.len() - label.len()..inline.text.len(), url));
                *pos += 1;
            }
            Event::InlineHtml(h) | Event::Html(h) => {
                push_run(&mut inline, h.len(), &st);
                inline.text.push_str(h);
                *pos += 1;
            }
            Event::TaskListMarker(done) => {
                let marker = if *done { "☑ " } else { "☐ " };
                push_run(&mut inline, marker.len(), &st);
                inline.text.push_str(marker);
                *pos += 1;
            }
            Event::FootnoteReference(f) => {
                let label = format!("[{f}]");
                push_run(&mut inline, label.len(), &st);
                inline.text.push_str(&label);
                *pos += 1;
            }
            _ => *pos += 1,
        }
    }
    inline
}

/// Block-level start tags that terminate bare (tight-list) inline content.
fn is_block_start(tag: &Tag) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::Item
            | Tag::List(_)
            | Tag::BlockQuote(_)
            | Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::CodeBlock(_)
            | Tag::HtmlBlock
            | Tag::FootnoteDefinition(_)
    )
}

/// Block-level end tags that terminate bare (tight-list) inline content.
fn is_block_end(end: &TagEnd) -> bool {
    matches!(
        end,
        TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::Item
            | TagEnd::List(_)
            | TagEnd::BlockQuote(_)
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::CodeBlock
            | TagEnd::HtmlBlock
            | TagEnd::FootnoteDefinition
    )
}

/// Render the flattened inline content as selectable text; links become
/// clickable ranges. `ord` is the block's place in the document selection.
pub(crate) fn inline_element(
    inline: Inline,
    id: gpui::ElementId,
    group: &gpui::Entity<crate::shared::selection_group::SelectionGroup>,
    ord: usize,
) -> AnyElement {
    if inline.text.is_empty() {
        return gpui::div().into_any_element();
    }
    crate::shared::selectable::SelectableText::new(id, inline.text)
        .runs(inline.runs)
        .links(inline.links)
        .group(group, ord)
        .into_any_element()
}

#[cfg(test)]
#[path = "inline_tests.rs"]
mod tests;
