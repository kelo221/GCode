//! Phone layout from Desktop's `WebRemoteControlMobileShell.tsx`: below 768 px
//! the sidebar and the chat are two full-screen pages instead of side by side.
//! Spec: REMOTE_CONTROL.md "Phone layout".

use crate::app::root::RootView;
use crate::shared::i18n::t;
use crate::shared::theme::{BORDER, FRAME, HOVER, I_ARROW_LEFT, TEXT, icon, ui_size};
use crate::shared::theme_colors::color as rgb;
use gpui::{AnyElement, ClickEvent, Context, Window, div, prelude::*, px};

/// Widest window that gets the phone layout (`max-width: 767px`).
pub(crate) const MOBILE_MAX_WIDTH: f32 = 767.;
/// Chat page header height (`h-11`).
const CHAT_HEADER_HEIGHT: f32 = 44.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MobilePage {
    /// Full-width sidebar: tasks, projects, settings.
    #[default]
    Home,
    /// Full-width workspace panel under a back header.
    Chat,
}

pub(crate) fn is_compact(window_width: f32) -> bool {
    window_width <= MOBILE_MAX_WIDTH
}

impl RootView {
    /// Only explicit task selection or creation opens the chat page;
    /// background session changes must not move the user off Home.
    pub(crate) fn show_chat_page(&mut self) {
        self.mobile_page = MobilePage::Chat;
    }

    pub(crate) fn show_home_page(&mut self, cx: &mut Context<Self>) {
        self.mobile_page = MobilePage::Home;
        cx.notify();
    }

    /// Android's back key: closes the topmost layer. Returns whether anything closed.
    pub(crate) fn handle_back(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.close_image_viewer(cx) {
            return true;
        }
        if self.quickpick_open {
            self.close_command_center(window, cx);
        } else if self.settings.open {
            self.close_settings(window, cx);
        } else if self.mobile_page == MobilePage::Chat {
            self.show_home_page(cx);
        } else {
            return false;
        }
        true
    }

    pub(crate) fn mobile_chat_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let back = div()
            .id("mobile-back-home")
            .size(px(32.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(HOVER)))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.show_home_page(cx)))
            .child(icon(I_ARROW_LEFT, 16., TEXT));
        let back = div().child(back);
        #[cfg(test)]
        let back = crate::app::test_support::track_children(back, vec!["mobile-back-home".into()]);
        div()
            .h(px(CHAT_HEADER_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .bg(rgb(FRAME))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(back)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(ui_size(13.)))
                    .text_color(rgb(TEXT))
                    .child(t("webRemoteControl.mobileShell.chatTitle")),
            )
            .children(self.mobile_agent_icons(cx))
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "mobile_shell_tests.rs"]
mod tests;
