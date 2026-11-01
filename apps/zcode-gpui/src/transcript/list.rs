//! Virtualized transcript: renders only the visible row window through gpui's
//! `list` element (PARITY.md M2 "Virtualization: 10k rows smooth"). Owns the
//! cached row index, follow-bottom tracking and scroll anchoring.

use crate::app::root::RootView;
use gpui::{
    AnyElement, Context, CursorStyle, InteractiveElement, IntoElement, ListAlignment, ListOffset,
    ListSizingBehavior, ListState, MouseButton, MouseDownEvent, ParentElement, ScrollEvent, Styled,
    div, px,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) const OVERDRAW: f32 = 800.;

/// Create the shared list state plus the follow-bottom flag. Appends only
/// auto-scroll while the flag is set. User scrolling writes it in exactly
/// one place per input (wheel: `sync_follow_after_wheel`, keys, autoscroll).
// 不再注册 list 的 scroll handler：它用滚动前的位置判断"视口底边落在最后一行内"，
// 最后一行是很长的日志/流式输出时，读者在行内阅读也会被判为到底；滚轮处理器对
// 纯横向事件（dy=0，触控板横向分量、手机触摸）直接返回，于是这个错误的 true
// 留下来，下一个流式增量把视图拉回最新消息。
pub(crate) fn new_list_state() -> (ListState, Arc<AtomicBool>) {
    let follow = Arc::new(AtomicBool::new(true));
    let state = ListState::new(0, ListAlignment::Top, px(OVERDRAW));
    (state, follow)
}

/// Where to put the reader after the row set was replaced (`Reset`
/// without following): the same row by id, else the first row after it.
/// Row ids are ascending (BTreeMap keys) in both slices.
// 原先按列表下标恢复：重连 snapshot 只带最新一窗的行，旧下标被截到新列表末尾，
// 读者就被带到最新消息。按 row id 找回同一行。
pub(crate) fn reanchor(prev_ids: &[u64], next_ids: &[u64], prev_top: ListOffset) -> ListOffset {
    let last = next_ids.len().saturating_sub(1);
    let Some(&row_id) = prev_ids.get(prev_top.item_ix) else {
        return ListOffset {
            item_ix: last,
            offset_in_item: px(0.),
        };
    };
    match next_ids.binary_search(&row_id) {
        Ok(ix) => ListOffset {
            item_ix: ix,
            offset_in_item: prev_top.offset_in_item,
        },
        Err(ix) => ListOffset {
            item_ix: ix.min(last),
            offset_in_item: px(0.),
        },
    }
}

/// How the cached row index changes between two renders.
#[derive(Debug, PartialEq)]
pub(crate) enum ListEdit {
    /// Same ids: nothing to splice (streaming may still grow the last row).
    Unchanged,
    /// `n` rows appended at the tail.
    Append(usize),
    /// `n` older rows paged in at the head (history).
    Prepend(usize),
    /// Anything else: rebuild the index.
    Reset,
}

pub(crate) fn plan_list_edit(prev: &[u64], next: &[u64], session_changed: bool) -> ListEdit {
    if session_changed {
        ListEdit::Reset
    } else if prev == next {
        ListEdit::Unchanged
    } else if next.len() > prev.len() && next.starts_with(prev) {
        ListEdit::Append(next.len() - prev.len())
    } else if !prev.is_empty() && next.len() > prev.len() && next.ends_with(prev) {
        ListEdit::Prepend(next.len() - prev.len())
    } else {
        ListEdit::Reset
    }
}

/// `offset_in_item` used to pin the tail. Larger than any row, so gpui's
/// layout pass ("rendered items do not fill the visible region") walks
/// upward and bottom-aligns the last row, even one taller than the
/// viewport. `scroll_to_reveal_item` would instead snap a tall last row to
/// its top when it already starts the viewport.
const PIN_TAIL_OFFSET: f32 = 1.0e7;

pub(crate) fn tail_pin(row_count: usize) -> Option<ListOffset> {
    (row_count > 0).then(|| ListOffset {
        item_ix: row_count - 1,
        offset_in_item: px(PIN_TAIL_OFFSET),
    })
}

impl RootView {
    /// Reconcile the cached row index with the conversation model. Called
    /// every render before the element tree is built.
    pub(crate) fn sync_list(&mut self, _row_count: usize, first_row_id: u64, cx: &Context<Self>) {
        let (active_sid, last_id, count) = {
            let state = self.state.read(cx);
            let conv = state.active_conversation();
            (
                state.active.clone(),
                conv.and_then(|c| c.rows.keys().next_back().copied())
                    .unwrap_or(0),
                conv.map_or(0, |c| c.rows.len()),
            )
        };
        // Row ids restart at 1 per conversation, so a session switch invalidates
        // the whole cached index even when counts/prefixes coincide.
        let session_changed = active_sid != self.last_session;
        let follow = self.follow_bottom.load(Ordering::Relaxed);
        if !session_changed
            && count == self.last_row_count
            && (first_row_id, last_id) == self.last_bounds
        {
            // Same rows, but a streaming row may have grown: keep the tail
            // pinned while following.
            if follow && let Some(pin) = tail_pin(count) {
                self.list_state.scroll_to(pin);
            }
            return;
        }

        let ids: Vec<u64> = self
            .state
            .read(cx)
            .active_conversation()
            .map(|c| c.rows.keys().copied().collect())
            .unwrap_or_default();
        // The index is the single source of the list's item count.
        let row_count = ids.len();
        let prev_count = self.row_index.len();
        // `reset` discards the logical scroll top, so capture it first.
        let prev_top = self.list_state.logical_scroll_top();
        let edit = plan_list_edit(&self.row_index, &ids, session_changed);
        let reading_anchor = (edit == ListEdit::Reset && !session_changed && !follow)
            .then(|| reanchor(&self.row_index, &ids, prev_top));
        match edit {
            ListEdit::Unchanged => {}
            ListEdit::Append(n) => self.list_state.splice(prev_count..prev_count, n),
            // Splicing at the head keeps measured heights and shifts the
            // logical scroll top by `n`, so the visible row stays in place
            // without the flash a full `reset` (all rows unmeasured) causes.
            ListEdit::Prepend(n) => self.list_state.splice(0..0, n),
            ListEdit::Reset => self.list_state.reset(row_count),
        }

        self.row_index = ids;
        self.last_row_count = row_count;
        self.last_first_row_id = first_row_id;
        self.last_bounds = (first_row_id, last_id);
        self.last_session = active_sid;
        if session_changed {
            // A new conversation opens pinned to its tail; an in-flight
            // autoscroll belongs to the old list.
            self.follow_bottom.store(true, Ordering::Relaxed);
            self.middle_scroll = None;
        }
        if row_count == 0 {
            return;
        }

        // These only mutate list state (consumed by the next layout), so they
        // are safe to call here during render.
        if session_changed || prev_count == 0 || follow {
            if let Some(pin) = tail_pin(row_count) {
                self.list_state.scroll_to(pin);
            }
        } else if let Some(anchor) = reading_anchor {
            // Replacement/truncation while reading history: stay on the same
            // row instead of snapping to the top after `reset`.
            self.list_state.scroll_to(anchor);
        }
    }

    /// The virtualized row window, centered on the reading column.
    pub(crate) fn transcript_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity();
        let state = self.list_state.clone();
        let is_middle_scrolling = self.middle_scroll.is_some();
        let jump = (!self.row_index.is_empty() && !self.follow_bottom.load(Ordering::Relaxed))
            .then(|| self.jump_to_latest_button(cx));
        div()
            .id("transcript-scroll-area")
            .relative()
            .size_full()
            .flex_1()
            .min_h_0()
            .track_focus(&self.transcript_focus)
            .cursor(if is_middle_scrolling {
                CursorStyle::ResizeUpDown
            } else {
                CursorStyle::Arrow
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    this.focus_transcript_on_click(window, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    this.handle_middle_down(ev, window, cx);
                }),
            )
            // Move/release/cancel for autoscroll are bound on the root view so
            // they still fire once the pointer leaves the transcript.
            .on_scroll(cx.listener(|this, ev: &ScrollEvent, _window, _cx| {
                this.sync_follow_after_wheel(ev.delta.pixel_delta(px(20.)).y);
            }))
            .child(
                gpui::list(state, move |ix, _window, app| {
                    entity.update(app, |view, cx| view.render_virtual_row(ix, cx))
                })
                .with_sizing_behavior(ListSizingBehavior::Auto)
                .size_full(),
            )
            .children(jump)
            .into_any_element()
    }

    /// Round "↓" over the bottom of the transcript while the reader has
    /// scrolled away from the newest row; a click returns and follows again.
    fn jump_to_latest_button(&self, cx: &mut Context<Self>) -> AnyElement {
        use crate::shared::theme::{BORDER, CARD, CARD_HOVER, I_CHEVRON_DOWN, MUTED, icon};
        use crate::shared::theme_colors::color as rgb;
        use gpui::StatefulInteractiveElement;
        let button = div()
            .id("jump-to-latest")
            .size(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(rgb(CARD))
            .border_1()
            .border_color(rgb(BORDER))
            .shadow_md()
            .cursor(CursorStyle::PointingHand)
            .hover(|s| s.bg(rgb(CARD_HOVER)))
            .tooltip(ely_gpui_component::primitives::Tooltip::text(
                crate::shared::i18n::label("Jump to latest", "跳到最新"),
            ))
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.scroll_transcript_to_tail();
                cx.notify();
            }))
            .child(icon(I_CHEVRON_DOWN, 16., MUTED));
        let wrapper = div()
            .absolute()
            .bottom_3()
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(button);
        #[cfg(test)]
        let wrapper =
            crate::app::test_support::track_children(wrapper, vec!["jump-to-latest".into()]);
        wrapper.into_any_element()
    }

    /// Single row lookup + render for the virtual list's render callback.
    pub(crate) fn render_virtual_row(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(&row_id) = self.row_index.get(ix) else {
            return div().into_any_element();
        };
        let state = self.state.read(cx);
        let Some(row) = state
            .active_conversation()
            .and_then(|c| c.rows.get(&row_id))
            .cloned()
        else {
            return div().into_any_element();
        };
        if matches!(row, crate::conversation::model::Row::TurnHeader { .. })
            && state.active_conversation().is_some_and(|c| {
                crate::transcript::noise_free::is_empty_finished_turn(&c.rows, row_id)
            })
        {
            return div().into_any_element();
        }
        let row_actions = !state.is_read_only_view()
            && state
                .active_conversation()
                .is_some_and(|c| c.log_epoch.is_some() && c.subscribed);
        use crate::transcript::noise_free::Place;
        let place = if crate::transcript::noise_free::enabled(cx) {
            self.fold_place(ix, cx)
        } else {
            Place::Show
        };
        let fold_line = match place {
            Place::Show => None,
            Place::Member { leader } => {
                let leader_id = self.row_index.get(leader).copied().unwrap_or(row_id);
                if !self.expanded_folds.contains(&leader_id) {
                    return div().into_any_element();
                }
                None
            }
            Place::Leader(summary) => {
                let expanded = self.expanded_folds.contains(&row_id);
                Some((
                    self.render_fold_line(row_id, summary, expanded, cx),
                    expanded,
                ))
            }
        };
        let content = match fold_line {
            Some((line, true)) => Some(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(line)
                    .children(self.transcript_row(&row, row_actions, cx))
                    .into_any_element(),
            ),
            Some((line, false)) => Some(line),
            None => self.transcript_row(&row, row_actions, cx),
        };
        // 无内容的行（空思考块、已配对的子代理行等）不能套 py_2，否则留下空白间隙。
        let Some(content) = content else {
            return div().into_any_element();
        };
        div()
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .w(px(self.conversation_column))
                    .min_w_0()
                    .py_2()
                    .child(content),
            )
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "list_tests.rs"]
mod tests;
