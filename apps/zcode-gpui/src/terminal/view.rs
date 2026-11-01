//! Terminal UI drawer: expandable bottom drawer matching desktop
//! AnimatedTerminalPanel parity. Renders colored grid rows, provides focus
//! handling, and auto-reports size changes via paint-phase probe.

use crate::app::root::RootView;
use crate::shared::theme_colors::color as rgb;
use crate::shared::{
    i18n::label,
    theme::{BORDER, CODE_BG, CODE_BORDER, HOVER, MUTED, PANEL, TEXT, ui_size},
};
use crate::terminal::grid::{self, GridRun, TERM_FG};
use gpui::{
    AnyElement, Context, CursorStyle, IntoElement, MouseButton, ParentElement, Styled, div,
    prelude::*, px,
};
use std::sync::{Arc, Mutex};

const FONT_SIZE: f32 = 11.5;

impl RootView {
    /// Render the terminal as an expandable bottom drawer.
    pub(crate) fn term_drawer(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.term.workspace.is_none() {
            return div()
                .p_3()
                .text_size(px(ui_size(12.)))
                .text_color(rgb(MUTED))
                .child(label("No workspace selected", "未选择工作区"))
                .into_any_element();
        }
        let metrics = crate::terminal::metrics::font_metrics(FONT_SIZE, cx);
        #[cfg(test)]
        let metrics = self.term.test_cell_metrics.unwrap_or(metrics);
        self.term.cell_metrics = metrics;
        let dims = self.term.dims;
        let rows = self
            .term
            .term
            .as_ref()
            .map(|term| grid::grid_rows(term.renderable_content(), TERM_FG, CODE_BG))
            .unwrap_or_default();
        let focused = self.term.focus.is_focused(window);
        let key_owner = self.terminal_owner();
        let scroll_owner = key_owner.clone();
        let middle_owner = key_owner.clone();
        let middle_up_owner = key_owner.clone();
        let middle_out_owner = key_owner.clone();
        let move_owner = key_owner.clone();
        let mut grid = div()
            .id("term-grid")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .bg(rgb(CODE_BG))
            .font_family(crate::shared::theme::MONO_FONT)
            .text_size(px(FONT_SIZE))
            .text_color(rgb(TERM_FG))
            .key_context("Terminal")
            .track_focus(&self.term.focus)
            .cursor(if self.term.middle_anchor.is_some() {
                CursorStyle::ResizeUpDown
            } else {
                CursorStyle::IBeam
            })
            .on_click(cx.listener(|this, _, window, cx| {
                window.focus(&this.term.focus, cx);
            }))
            .on_key_down(
                cx.listener(move |this, ev: &gpui::KeyDownEvent, window, cx| {
                    if !this.terminal_owner_current(&key_owner, cx) {
                        cx.stop_propagation();
                        return;
                    }
                    if !this.handle_navigation_key(ev, window, cx) {
                        this.term_key(&ev.keystroke, cx);
                    }
                    cx.stop_propagation();
                }),
            )
            .on_scroll(
                cx.listener(move |this, ev: &gpui::ScrollEvent, _window, cx| {
                    if !this.terminal_owner_current(&scroll_owner, cx) {
                        return;
                    }
                    let delta = ev.delta.pixel_delta(px(20.));
                    let lines = (delta.y / px(metrics.height)).round() as i32;
                    if lines != 0
                        && let Some(term) = this.term.term.as_mut()
                    {
                        term.scroll_display(alacritty_terminal::grid::Scroll::Delta(lines));
                        cx.notify();
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, ev: &gpui::MouseDownEvent, _window, cx| {
                    if !this.terminal_owner_current(&middle_owner, cx) {
                        return;
                    }
                    this.term.selecting = false;
                    this.term.middle_anchor = Some(ev.position);
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |this, _ev: &gpui::MouseUpEvent, _window, cx| {
                    if !this.terminal_owner_current(&middle_up_owner, cx) {
                        return;
                    }
                    this.term.middle_anchor = None;
                    cx.notify();
                }),
            )
            // Releasing outside the grid must also end the drag, or the next
            // hover would keep scrolling with a stale anchor.
            .on_mouse_up_out(
                MouseButton::Middle,
                cx.listener(move |this, _ev: &gpui::MouseUpEvent, _window, cx| {
                    if !this.terminal_owner_current(&middle_out_owner, cx) {
                        return;
                    }
                    if this.term.middle_anchor.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(
                cx.listener(move |this, ev: &gpui::MouseMoveEvent, _window, cx| {
                    if !this.terminal_owner_current(&move_owner, cx) {
                        return;
                    }
                    if let Some(anchor) = this.term.middle_anchor {
                        let dy = ev.position.y - anchor.y;
                        let lines = -(dy / px(metrics.height)).round() as i32;
                        if lines != 0
                            && let Some(term) = this.term.term.as_mut()
                        {
                            term.scroll_display(alacritty_terminal::grid::Scroll::Delta(lines));
                            this.term.middle_anchor = Some(ev.position);
                            cx.notify();
                        }
                    }
                }),
            )
            .children(rows.into_iter().map(|row| term_row(row, metrics)));
        grid = self.terminal_selection_handlers(grid, cx);
        if !focused {
            grid = grid.child(
                div()
                    .p_1()
                    .text_size(px(ui_size(10.5)))
                    .text_color(rgb(MUTED))
                    .child(label("Click to focus the terminal", "点击以聚焦终端")),
            );
        }

        div()
            .h(px(240.))
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(rgb(BORDER))
            .bg(rgb(PANEL))
            // Header bar of the bottom terminal drawer.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .h(px(28.))
                    .border_b_1()
                    .border_color(rgb(CODE_BORDER))
                    .child(
                        div()
                            .text_size(px(ui_size(12.)))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(rgb(TEXT))
                            .child(label("Terminal", "终端")),
                    )
                    .child(
                        div()
                            .text_size(px(ui_size(11.)))
                            .text_color(rgb(MUTED))
                            .child(if self.term.exited {
                                label(
                                    "process exited — reopen to restart",
                                    "进程已退出 — 重新打开以重启",
                                )
                                .to_string()
                            } else {
                                format!(
                                    "{}×{} — {}",
                                    dims.cols,
                                    dims.rows,
                                    label("local shell", "本地 shell")
                                )
                            }),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("term-drawer-close")
                            .px_1p5()
                            .py_0p5()
                            .rounded_sm()
                            .text_size(px(ui_size(11.)))
                            .text_color(rgb(MUTED))
                            .cursor(CursorStyle::PointingHand)
                            .hover(|h| h.bg(rgb(HOVER)).text_color(rgb(TEXT)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.term_open = false;
                                cx.notify();
                            }))
                            .child("✕"),
                    ),
            )
            .child(
                div()
                    .p_1p5()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(measured_grid(
                        grid,
                        &self.term.pending_size,
                        &self.term.grid_bounds,
                    )),
            )
            .into_any_element()
    }
}

fn term_row(runs: Vec<GridRun>, metrics: crate::terminal::metrics::CellMetrics) -> gpui::Div {
    // 固定 cell 几何与 hit-test/PTY resize 共用，不能用字体宽度推算可变 run 的选区。
    let mut row = div().relative().h(px(metrics.height)).flex_none();
    for run in runs {
        let mut span = div()
            .absolute()
            .left(px(run.column as f32 * metrics.width))
            .w(px(run.columns as f32 * metrics.width))
            .h(px(metrics.height))
            .line_height(px(metrics.height))
            .overflow_hidden()
            .text_color(gpui::rgb(run.fg))
            .child(run.text.clone());
        if let Some(bg) = run.bg {
            span = span.bg(gpui::rgb(bg));
        }
        if run.bold {
            span = span.font_weight(gpui::FontWeight::BOLD);
        }
        if run.italic {
            span = span.italic();
        }
        row = row.child(span);
    }
    row
}

fn measured_grid(
    grid: gpui::Stateful<gpui::Div>,
    pending: &Arc<Mutex<Option<(f32, f32)>>>,
    grid_bounds: &Arc<Mutex<Option<gpui::Bounds<gpui::Pixels>>>>,
) -> gpui::Div {
    let pending = pending.clone();
    let grid_bounds = grid_bounds.clone();
    // 测量真实可交互 grid，叠加的 canvas 会挡住选区鼠标事件并产生不同的 bounds。
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .child(grid)
        .on_children_prepainted(move |bounds, _, _| {
            let Some(bounds) = bounds.first().copied() else {
                return;
            };
            if let Ok(mut recorded) = grid_bounds.lock() {
                *recorded = Some(bounds)
            }
            if let Ok(mut p) = pending.lock() {
                *p = Some((
                    f64::from(bounds.size.width) as f32,
                    f64::from(bounds.size.height) as f32,
                ));
            }
        })
}
