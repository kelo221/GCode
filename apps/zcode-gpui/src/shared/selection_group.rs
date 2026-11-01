//! One selection shared by several text blocks (a whole markdown document),
//! so a drag runs from one paragraph or list item into the next. Blocks are
//! `SelectableText`s that joined the group; the element spanning them all is
//! wrapped with [`selection_owner`].

use crate::shared::selectable::{index_at, snap, word_at};
use gpui::prelude::FluentBuilder;
use gpui::{
    App, Bounds, ClipboardItem, Context, DispatchPhase, Div, ElementId, Entity, FocusHandle,
    InteractiveElement, MouseButton, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point,
    SharedString, Stateful, Styled, TextLayout, Window, canvas,
};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

/// (block ordinal, byte offset in that block). Ordinals follow document
/// order, so tuple order is reading order.
type Pos = (usize, usize);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Granularity {
    Char,
    Word,
    Block,
}

/// A block as last painted: hit-testing and copy read these.
pub(crate) struct Slot {
    pub(crate) text: SharedString,
    pub(crate) layout: TextLayout,
    pub(crate) bounds: Bounds<Pixels>,
}

pub(crate) struct SelectionGroup {
    pub(crate) focus: FocusHandle,
    anchor: Pos,
    head: Pos,
    dragging: bool,
    granularity: Granularity,
    /// Word or block that started a double/triple-click drag; the selection
    /// always keeps it while extending in either direction.
    anchor_span: (Pos, Pos),
    pub(crate) slots: Rc<RefCell<Vec<Option<Slot>>>>,
}

impl SelectionGroup {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            anchor: (0, 0),
            head: (0, 0),
            dragging: false,
            granularity: Granularity::Char,
            anchor_span: ((0, 0), (0, 0)),
            slots: Rc::default(),
        }
    }

    /// The group stored under `key` in the current element scope.
    pub(crate) fn keyed(
        key: impl Into<ElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        window.use_keyed_state(key, cx, |_, cx| Self::new(cx))
    }

    /// Forget last frame's blocks; the blocks painted this frame re-register.
    pub(crate) fn begin_frame(&self) {
        self.slots.borrow_mut().clear();
    }

    fn ordered(&self) -> (Pos, Pos) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    pub(crate) fn block_range(&self, ord: usize, text: &str) -> Range<usize> {
        let (start, end) = self.ordered();
        block_range(start, end, ord, text)
    }

    fn slot_text(&self, ord: usize) -> Option<SharedString> {
        self.slots
            .borrow()
            .get(ord)
            .and_then(|s| s.as_ref().map(|s| s.text.clone()))
    }

    pub(crate) fn press(&mut self, hit: Pos, text: &str, click_count: usize, shift: bool) {
        self.dragging = true;
        let (start, end) = match click_count {
            0 | 1 => {
                self.granularity = Granularity::Char;
                if !shift {
                    self.anchor = hit;
                }
                self.head = hit;
                return;
            }
            2 => {
                self.granularity = Granularity::Word;
                let w = word_at(text, hit.1);
                ((hit.0, w.start), (hit.0, w.end))
            }
            _ => {
                self.granularity = Granularity::Block;
                ((hit.0, 0), (hit.0, text.len()))
            }
        };
        self.anchor_span = (start, end);
        (self.anchor, self.head) = (start, end);
    }

    fn extend_to(&mut self, hit: Pos) {
        if self.granularity == Granularity::Char {
            self.head = hit;
            return;
        }
        let span = match self.slot_text(hit.0) {
            Some(text) if self.granularity == Granularity::Word => word_at(&text, hit.1),
            Some(text) => 0..text.len(),
            None => hit.1..hit.1,
        };
        let (span_start, span_end) = self.anchor_span;
        if hit < span_start {
            self.anchor = span_end;
            self.head = (hit.0, span.start);
        } else {
            self.anchor = span_start;
            self.head = (hit.0, span.end).max(span_end);
        }
    }

    /// Block and offset under `p`. Between blocks the pointer counts as the end
    /// of the block above; above everything as the start of the first block.
    fn hit_test(&self, p: Point<Pixels>) -> Option<Pos> {
        let slots = self.slots.borrow();
        let live = || {
            slots
                .iter()
                .enumerate()
                .filter_map(|(i, s)| s.as_ref().map(|s| (i, s)))
        };
        // Rows covering `y`; side-by-side blocks (table cells) pick the closest.
        let mut best: Option<(usize, &Slot, Pixels)> = None;
        for (i, s) in live() {
            let b = s.bounds;
            if p.y < b.top() || p.y > b.bottom() {
                continue;
            }
            let dx = if p.x < b.left() {
                b.left() - p.x
            } else if p.x > b.right() {
                p.x - b.right()
            } else {
                Pixels::ZERO
            };
            if best.is_none_or(|(_, _, d)| dx < d) {
                best = Some((i, s, dx));
            }
        }
        if let Some((i, s, _)) = best {
            return Some((i, index_at(&s.layout, &s.text, p)));
        }
        let mut above = None;
        let mut first = None;
        for (i, s) in live() {
            first.get_or_insert(i);
            if s.bounds.bottom() < p.y {
                above = Some((i, s.text.len()));
            }
        }
        above.or(first.map(|i| (i, 0)))
    }

    fn selected_text(&self) -> String {
        let (start, end) = self.ordered();
        if start == end {
            return String::new();
        }
        let slots = self.slots.borrow();
        let mut parts: Vec<&str> = Vec::new();
        for ord in start.0..=end.0 {
            if let Some(Some(s)) = slots.get(ord) {
                let r = block_range(start, end, ord, &s.text);
                if !r.is_empty() {
                    parts.push(&s.text[r]);
                }
            }
        }
        parts.join("\n")
    }

    fn select_all(&mut self) {
        let slots = self.slots.borrow();
        let mut live = slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|s| (i, s.text.len())));
        let Some(first) = live.next() else {
            return;
        };
        let last = live.next_back().unwrap_or(first);
        drop(slots);
        (self.anchor, self.head) = ((first.0, 0), last);
    }
}

/// Part of block `ord` inside the ordered selection `start..end`.
fn block_range(start: Pos, end: Pos, ord: usize, text: &str) -> Range<usize> {
    if ord < start.0 || ord > end.0 {
        return 0..0;
    }
    let s = if ord == start.0 {
        snap(text, start.1)
    } else {
        0
    };
    let e = if ord == end.0 {
        snap(text, end.1)
    } else {
        text.len()
    };
    s..e.max(s)
}

/// Right-click menu for selectable text: Copy (the selection), Select All,
/// and an extra copy item (label, text) for the source, e.g. the Markdown.
pub(crate) fn selection_menu(
    group: &Entity<SelectionGroup>,
    source: Option<(&'static str, String)>,
    cx: &App,
) -> ely_gpui_component::menus::Menu {
    use ely_gpui_component::menus::{Menu, MenuItem};
    let label = crate::shared::i18n::label;
    let primary = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    let selected = {
        let g = group.read(cx);
        g.anchor != g.head
    };
    let (copy, all) = (group.clone(), group.clone());
    let menu = Menu::new()
        .item(
            MenuItem::new(label("Copy", "复制"))
                .keys(format!("{primary}-c"))
                .disabled(!selected)
                .on_click(move |_, cx| {
                    let text = copy.read(cx).selected_text();
                    if !text.is_empty() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }
                }),
        )
        .item(
            MenuItem::new(label("Select All", "全选"))
                .keys(format!("{primary}-a"))
                .on_click(move |window, cx| {
                    all.update(cx, |g, cx| {
                        window.focus(&g.focus, cx);
                        g.select_all();
                        cx.notify();
                    });
                }),
        );
    match source {
        Some((name, text)) => menu
            .separator()
            .item(MenuItem::new(name).on_click(move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
            })),
        None => menu,
    }
}

/// Focus, copy / select-all keys, click-outside clearing and the window-level
/// drag listeners for `group`, attached to the element that spans all of its
/// blocks.
pub(crate) fn selection_owner(
    root: Stateful<Div>,
    group: &Entity<SelectionGroup>,
    cx: &App,
) -> Stateful<Div> {
    let (focus, dragging) = {
        let g = group.read(cx);
        (g.focus.clone(), g.dragging)
    };
    let (out, keys, listeners) = (group.clone(), group.clone(), group.clone());
    root.track_focus(&focus)
        .on_mouse_down_out(move |_, _, cx| {
            out.update(cx, |g, cx| {
                g.dragging = false;
                if g.anchor != g.head {
                    g.anchor = g.head;
                    cx.notify();
                }
            });
        })
        .on_key_down(move |ev, _, cx| {
            let stroke = &ev.keystroke;
            if stroke.key == "escape" && !stroke.modifiers.modified() {
                // Only a live selection takes Escape; otherwise it still
                // closes the image viewer, settings and the like.
                let had = keys.update(cx, |g, cx| {
                    let had = g.anchor != g.head;
                    g.anchor = g.head;
                    cx.notify();
                    had
                });
                if had {
                    cx.stop_propagation();
                }
                return;
            }
            if !stroke.modifiers.secondary(cx) {
                return;
            }
            match stroke.key.as_str() {
                "c" => {
                    let text = keys.read(cx).selected_text();
                    if !text.is_empty() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                        cx.stop_propagation();
                    }
                }
                "a" => {
                    keys.update(cx, |g, cx| {
                        g.select_all();
                        cx.notify();
                    });
                    cx.stop_propagation();
                }
                _ => {}
            }
        })
        // Window-level listeners while dragging: element mouse-move only
        // fires over the element, so a drag past its edge would stall. The
        // owner hit-tests every block, so the drag can move between blocks.
        .when(dragging, |el| {
            el.child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        let (drag, up) = (listeners.clone(), listeners);
                        window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            if ev.pressed_button != Some(MouseButton::Left) {
                                drag.update(cx, |g, _| g.dragging = false);
                                return;
                            }
                            let Some(hit) = drag.read(cx).hit_test(ev.position) else {
                                return;
                            };
                            drag.update(cx, |g, cx| {
                                let before = (g.anchor, g.head);
                                g.extend_to(hit);
                                if (g.anchor, g.head) != before {
                                    cx.notify();
                                }
                            });
                        });
                        window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Bubble && ev.button == MouseButton::Left {
                                up.update(cx, |g, cx| {
                                    g.dragging = false;
                                    cx.notify();
                                });
                            }
                        });
                    },
                )
                .absolute()
                .size_0(),
            )
        })
}
