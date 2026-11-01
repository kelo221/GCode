//! Full-window viewer for tool-result images. Thumbnails in tool cards are too
//! small to read screenshots; the viewer fits the image to the window, and
//! "Actual size" shows it 1:1 in a scrollable area. Esc, Android back or a
//! click on the backdrop closes it.

use crate::app::root::RootView;
use crate::shared::i18n::label;
use crate::shared::theme::{BORDER, CARD, CARD_HOVER, MUTED, TEXT, ui_size};
use crate::shared::theme_colors::color as rgb;
use gpui::{
    AnyElement, ClickEvent, Context, CursorStyle, MouseButton, ObjectFit, StyledImage, Window, div,
    img, prelude::*, px, rgba,
};

const MARGIN: f32 = 24.;
const HEADER: f32 = 40.;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Viewer {
    pub key: String,
    /// 1:1 device pixels in a scroll area instead of fitting the window.
    pub actual: bool,
}

/// Logical size of the image shown 1:1 on a display with `scale` device
/// pixels per logical pixel, and its size fitted into `avail` without enlarging.
pub(crate) fn viewer_sizes(
    width: u32,
    height: u32,
    scale: f32,
    avail: (f32, f32),
) -> ((f32, f32), (f32, f32)) {
    let scale = if scale > 0. { scale } else { 1. };
    let natural = (width.max(1) as f32 / scale, height.max(1) as f32 / scale);
    let k = (avail.0.max(1.) / natural.0)
        .min(avail.1.max(1.) / natural.1)
        .min(1.);
    (natural, (natural.0 * k, natural.1 * k))
}

fn chip(id: &'static str, text: String) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_md()
        .bg(rgb(CARD))
        .border_1()
        .border_color(rgb(BORDER))
        .text_size(px(ui_size(12.)))
        .text_color(rgb(TEXT))
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.bg(rgb(CARD_HOVER)))
        .child(text)
}

impl RootView {
    pub(crate) fn open_image_viewer(&mut self, key: String, cx: &mut Context<Self>) {
        self.tool_images.viewer = Some(Viewer { key, actual: false });
        cx.notify();
    }

    /// `true` when a viewer was open.
    pub(crate) fn close_image_viewer(&mut self, cx: &mut Context<Self>) -> bool {
        let was_open = self.tool_images.viewer.take().is_some();
        if was_open {
            cx.notify();
        }
        was_open
    }

    fn toggle_image_viewer_size(&mut self, cx: &mut Context<Self>) {
        if let Some(v) = self.tool_images.viewer.as_mut() {
            v.actual = !v.actual;
            cx.notify();
        }
    }

    pub(crate) fn render_image_viewer(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let viewer = self.tool_images.viewer.as_ref()?;
        let preview = self.tool_images.loaded(&viewer.key)?;
        let viewport = window.viewport_size();
        let avail = (
            f32::from(viewport.width) - 2. * MARGIN,
            f32::from(viewport.height) - 2. * MARGIN - HEADER,
        );
        let (natural, fitted) =
            viewer_sizes(preview.width, preview.height, window.scale_factor(), avail);
        let image = img(gpui::ImageSource::Render(preview.image.clone()))
            .object_fit(ObjectFit::Contain)
            .cursor(CursorStyle::PointingHand);
        let toggle_label = if viewer.actual {
            label("Fit to window", "适应窗口")
        } else {
            label("Actual size", "实际大小")
        };
        // Clicks on the image or header must not reach the backdrop's close.
        let keep_open = cx.listener(|_, _: &gpui::MouseDownEvent, _, cx| cx.stop_propagation());
        let body = if viewer.actual {
            div()
                .id("image-viewer-scroll")
                .max_w(px(avail.0))
                .max_h(px(avail.1))
                .overflow_scroll()
                .child(image.w(px(natural.0)).h(px(natural.1)))
        } else {
            div()
                .id("image-viewer-fit")
                .child(image.w(px(fitted.0)).h(px(fitted.1)))
        }
        .on_mouse_down(MouseButton::Left, keep_open)
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.toggle_image_viewer_size(cx);
        }));
        let header = div()
            .h(px(HEADER))
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _: &gpui::MouseDownEvent, _, cx| cx.stop_propagation()),
            )
            .child(
                div()
                    .text_size(px(ui_size(12.)))
                    .text_color(rgb(MUTED))
                    .child(format!("{} × {}", preview.width, preview.height)),
            )
            .child(
                chip("image-viewer-size", toggle_label.to_string()).on_click(
                    cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_image_viewer_size(cx)),
                ),
            )
            .child(
                chip("image-viewer-close", label("Close", "关闭").to_string()).on_click(
                    cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.close_image_viewer(cx);
                    }),
                ),
            );
        Some(
            div()
                .id("image-viewer")
                .absolute()
                .inset_0()
                .p(px(MARGIN))
                .bg(rgba(0x000000d9))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &gpui::MouseDownEvent, _, cx| {
                        this.close_image_viewer(cx);
                    }),
                )
                .on_scroll(|_, _, cx| cx.stop_propagation())
                .child(header)
                .child(body)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::viewer_sizes;

    #[test]
    fn phone_screenshot_fits_the_window_height_and_shows_1_to_1_logical_size() {
        let (natural, fitted) = viewer_sizes(1080, 2400, 1.5, (1200., 800.));
        assert_eq!(natural, (720., 1600.));
        assert_eq!(fitted, (360., 800.));
    }

    #[test]
    fn small_images_are_never_enlarged() {
        let (natural, fitted) = viewer_sizes(200, 100, 1., (1200., 800.));
        assert_eq!(natural, fitted);
    }

    #[test]
    fn viewer_opens_on_a_loaded_image_and_escape_or_back_closes_it() {
        use crate::app::{root::RootView, store::AppState, test_support::RUNTIME_TEST_LOCK};
        use gpui::{TestApp, px, size};
        use std::sync::Arc;
        let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut app = TestApp::with_text_system_and_assets(
            Arc::new(gpui::NoopTextSystem::new()),
            Arc::new(ely_gpui_component::Assets),
        );
        app.update(|cx| {
            ely_gpui_component::init(cx);
            crate::shared::preferences::Preferences::install_at(
                Default::default(),
                std::env::temp_dir().join(uuid::Uuid::now_v7().to_string()),
                Arc::new(|| true),
                cx,
            );
        });
        let state = app.new_entity(AppState::for_test);
        let mut window = app.open_window(|_, cx| RootView::new(state.clone(), cx));
        window.simulate_resize(size(px(1280.), px(820.)));
        window.update(|view, window, cx| {
            window.bounds_changed(cx);
            let preview = crate::files::image_preview::ImagePreview {
                image: Arc::new(gpui::RenderImage::new(vec![image::Frame::new(
                    image::RgbaImage::new(4, 8),
                )])),
                width: 4,
                height: 8,
            };
            view.tool_images.insert_loaded("k", preview);
            view.open_image_viewer("k".into(), cx);
            let focus = view.state.read(cx).composer.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        window.draw();
        window.simulate_keystroke("escape");
        window.update(|view, _, _| assert_eq!(view.tool_images.viewer, None));
        window.update(|view, window, cx| {
            view.open_image_viewer("k".into(), cx);
            assert!(view.handle_back(window, cx));
            assert_eq!(view.tool_images.viewer, None);
        });
    }
}
