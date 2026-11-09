//! Block-level markdown → gpui elements (headings, paragraphs, lists, quotes,
//! tables, rules, fenced code). Assistant rows render through
//! `render_markdown`; streaming re-parses per frame (µs-scale for chat sizes).
//!
//! Spec: PARITY.md M2 "Streaming Markdown renderer".

pub(crate) mod code;
pub(crate) mod file_exists;
pub(crate) mod file_ref;
mod images;
pub(crate) mod inline;
mod passive;
mod table;

use crate::shared::markdown::code::code_block;
use crate::shared::markdown::inline::{Style, inline_element, render_inlines};
use crate::shared::markdown::table::render_table;
use crate::shared::selection_group::{SelectionGroup, selection_menu, selection_owner};
use crate::shared::theme::ui_size;
use crate::shared::theme_colors::color as rgb;
use ely_gpui_component::menus::ContextMenu;
use gpui::{AnyElement, Entity, IntoElement, ParentElement, Styled, div, prelude::*, px};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

pub(crate) use images::image_targets;

const BASE_TEXT: f32 = 13.5;

pub fn render_markdown(text: &str, id_base: u64, streaming: bool) -> AnyElement {
    render_markdown_with_policy(text, id_base, streaming, false)
}

pub(crate) fn render_passive_markdown(text: &str, id_base: u64) -> AnyElement {
    render_markdown_with_policy(text, id_base, false, true)
}

fn render_markdown_with_policy(
    text: &str,
    id_base: u64,
    streaming: bool,
    passive: bool,
) -> AnyElement {
    let parsed = images::windows_image_paths_to_slashes(text);
    let body = if streaming {
        format!("{parsed} ▍")
    } else {
        parsed.into_owned()
    };
    MarkdownDoc {
        body,
        source: text.to_string(),
        id_base,
        passive,
    }
    .into_any_element()
}

/// One document: every text block joins one selection, so a drag can run
/// across paragraphs, list items, tables and code.
#[derive(IntoElement)]
struct MarkdownDoc {
    body: String,
    /// The Markdown as written, for Copy as Markdown.
    source: String,
    id_base: u64,
    passive: bool,
}

impl RenderOnce for MarkdownDoc {
    fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl IntoElement {
        let id_base = self.id_base;
        let mut opts = Options::empty();
        opts.insert(
            Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
        );
        let events: Vec<Event> = if self.passive {
            passive::events(&self.body, opts)
        } else {
            Parser::new_ext(&self.body, opts).collect()
        };
        let group = SelectionGroup::keyed(
            gpui::ElementId::NamedInteger("md-sel".into(), id_base),
            window,
            cx,
        );
        group.read(cx).begin_frame();
        let mut pos = 0usize;
        let mut block_ix = 0usize;
        // Link hitbox ids are seeded by row id so they stay unique across rows.
        let mut ids = id_base.wrapping_mul(4096);
        let children = blocks(
            &events,
            &mut pos,
            id_base,
            &mut block_ix,
            0,
            &mut ids,
            &group,
        );

        let menu = selection_menu(
            &group,
            Some((
                crate::shared::i18n::label("Copy as Markdown", "复制为 Markdown"),
                self.source,
            )),
            cx,
        );
        let root = div()
            // Block ids restart at 1 per document; this keeps them apart
            // between documents.
            .id(gpui::ElementId::NamedInteger("md".into(), id_base))
            .w_full()
            .text_size(px(ui_size(BASE_TEXT)))
            .text_color(rgb(crate::shared::theme::TEXT))
            .child(
                ContextMenu::new(
                    gpui::ElementId::NamedInteger("md-ctx".into(), id_base),
                    menu,
                )
                .child(div().w_full().flex().flex_col().gap_1().children(children)),
            );
        selection_owner(root, &group, cx)
    }
}

/// Consume block-level events until the slice ends (top level) or a matching
/// container end would be reached (nested lists / quotes stop at their End).
fn blocks(
    events: &[Event],
    pos: &mut usize,
    id_base: u64,
    block_ix: &mut usize,
    depth: usize,
    ids: &mut u64,
    group: &Entity<SelectionGroup>,
) -> Vec<AnyElement> {
    let mut out: Vec<AnyElement> = Vec::new();
    while *pos < events.len() {
        match &events[*pos] {
            // 顶层没有调用方容器：多余的 End 只能是遍历错位，跳过它而不是丢掉后文。
            Event::End(_) if depth == 0 => *pos += 1,
            Event::End(_) => break, // caller's container end
            Event::Start(Tag::Paragraph) => {
                *pos += 1;
                let inline =
                    render_inlines(events, pos, Some(&TagEnd::Paragraph), &Style::default());
                *block_ix += 1;
                out.push(
                    div()
                        .w_full()
                        .child(inline_element(
                            inline,
                            gpui::ElementId::NamedInteger("md-blk".into(), *block_ix as u64),
                            group,
                            *block_ix,
                        ))
                        .into_any_element(),
                );
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let size = match level {
                    pulldown_cmark::HeadingLevel::H1 => 18.,
                    pulldown_cmark::HeadingLevel::H2 => 16.,
                    pulldown_cmark::HeadingLevel::H3 => 14.,
                    _ => 14.,
                };
                let end = TagEnd::Heading(*level);
                *pos += 1;
                let heading_style = Style {
                    bold: true,
                    ..Style::default()
                };
                let inline = render_inlines(events, pos, Some(&end), &heading_style);
                *block_ix += 1;
                out.push(
                    div()
                        .w_full()
                        .mt_1()
                        .text_size(px(ui_size(size)))
                        .child(inline_element(
                            inline,
                            gpui::ElementId::NamedInteger("md-blk".into(), *block_ix as u64),
                            group,
                            *block_ix,
                        ))
                        .into_any_element(),
                );
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(info) => {
                        info.split([' ', ',']).next().unwrap_or("").to_string()
                    }
                    pulldown_cmark::CodeBlockKind::Indented => String::new(),
                };
                let mut code = String::new();
                *pos += 1;
                while *pos < events.len() {
                    match &events[*pos] {
                        Event::Text(t) => {
                            code.push_str(t);
                            *pos += 1;
                        }
                        Event::End(TagEnd::CodeBlock) => {
                            *pos += 1;
                            break;
                        }
                        _ => *pos += 1,
                    }
                }
                *block_ix += 1;
                let id = (id_base as u128) << 8 | *block_ix as u128 & 0xff;
                let id64 = u64::try_from(id).unwrap_or(id_base);
                out.push(code_block(
                    id64,
                    code.trim_end_matches('\n'),
                    &lang,
                    Some((group, *block_ix)),
                ));
            }
            Event::Start(Tag::List(start)) => {
                let start_num = *start;
                let end = TagEnd::List(start_num.is_some());
                *pos += 1;
                out.push(render_list(
                    events, pos, id_base, block_ix, start_num, depth, end, ids, group,
                ));
            }
            Event::Start(Tag::BlockQuote(_)) => {
                *pos += 1;
                let inner = blocks(events, pos, id_base, block_ix, depth + 1, ids, group);
                if matches!(events.get(*pos), Some(Event::End(TagEnd::BlockQuote(_)))) {
                    *pos += 1;
                }
                out.push(
                    div()
                        .w_full()
                        .pl_3()
                        .ml_1()
                        .border_l_2()
                        .border_color(rgb(crate::shared::theme::BORDER))
                        .text_color(rgb(crate::shared::theme::MUTED))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children(inner)
                        .into_any_element(),
                );
            }
            Event::Rule => {
                *pos += 1;
                out.push(
                    div()
                        .w_full()
                        .h(px(1.))
                        .bg(rgb(crate::shared::theme::BORDER))
                        .into_any_element(),
                );
            }
            Event::Start(Tag::Table(aligns)) => {
                let aligns = aligns.clone();
                *pos += 1;
                out.push(render_table(events, pos, &aligns, block_ix, group));
            }
            Event::Start(Tag::HtmlBlock) => {
                // v1: raw HTML blocks render as muted plain text.
                if let Some(Event::Html(h)) = events.get(*pos) {
                    out.push(
                        div()
                            .w_full()
                            .text_color(rgb(crate::shared::theme::MUTED))
                            .child(h.to_string())
                            .into_any_element(),
                    );
                }
                *pos += 1;
            }
            Event::Start(Tag::FootnoteDefinition(_)) => {
                *pos += 1;
                blocks(events, pos, id_base, block_ix, depth + 1, ids, group);
            }
            // Tight-list items carry bare inline events with no Paragraph
            // wrapper; render them as one inline paragraph.
            Event::Text(_)
            | Event::Code(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::InlineHtml(_)
            | Event::Html(_)
            | Event::TaskListMarker(_)
            | Event::FootnoteReference(_) => {
                let inline = render_inlines(events, pos, None, &Style::default());
                *block_ix += 1;
                out.push(
                    div()
                        .w_full()
                        .child(inline_element(
                            inline,
                            gpui::ElementId::NamedInteger("md-blk".into(), *block_ix as u64),
                            group,
                            *block_ix,
                        ))
                        .into_any_element(),
                );
            }
            _ => *pos += 1,
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn render_list(
    events: &[Event],
    pos: &mut usize,
    id_base: u64,
    block_ix: &mut usize,
    start_num: Option<u64>,
    depth: usize,
    end: TagEnd,
    ids: &mut u64,
    group: &Entity<SelectionGroup>,
) -> AnyElement {
    let mut items: Vec<AnyElement> = Vec::new();
    let mut index = start_num.unwrap_or(1);
    let indent = depth.min(4) as f32 * 14.;
    while *pos < events.len() {
        match &events[*pos] {
            Event::End(e) if *e == end => {
                *pos += 1;
                break;
            }
            Event::Start(Tag::Item) => {
                *pos += 1;
                let marker = if start_num.is_some() {
                    format!("{index}.")
                } else {
                    "•".to_string()
                };
                index += 1;
                let mut item = div().w_full().flex().gap_1p5();
                item = item.child(
                    div()
                        .min_w(px(14.))
                        .text_color(rgb(crate::shared::theme::MUTED))
                        .child(marker),
                );
                item = item.child(div().flex_1().min_w_0().flex().flex_col().gap_1().children(
                    blocks(events, pos, id_base, block_ix, depth + 1, ids, group),
                ));
                if matches!(events.get(*pos), Some(Event::End(TagEnd::Item))) {
                    *pos += 1;
                }
                items.push(item.ml(px(indent)).into_any_element());
            }
            _ => *pos += 1,
        }
    }
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_0p5()
        .children(items)
        .into_any_element()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
