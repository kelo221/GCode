//! Settings → Model settings: the desktop "Model Provider" two-column layout
//! (provider list + detail). See PROVIDER_SETTINGS.md.

use crate::app::root::RootView;
use crate::provider_settings::{Provider, ProviderView, Status};
use crate::shared::i18n::label;
use crate::shared::theme::{active_theme, font_size_sm, ui_size};
use ely_gpui_component::buttons::{Button, ButtonVariant, IconButton};
use ely_gpui_component::primitives::IconName;
use ely_gpui_component::theme::ControlSize;
use gpui::{AnyElement, Context, IntoElement, Rgba, SharedString, Window, div, prelude::*, px};

#[cfg(test)]
pub(crate) use crate::app::settings_models_catalog::filtered_model_groups;

const NAV_WIDTH: f32 = 224.;
const CARD_MIN_HEIGHT: f32 = 576.;

pub(crate) fn status_color(status: Status) -> Rgba {
    let theme = active_theme();
    match status {
        Status::Ready => theme.success,
        Status::Unavailable => theme.warning,
        Status::Disabled => theme.subtlest,
    }
}

pub(crate) fn status_dot(status: Status) -> impl IntoElement {
    div()
        .flex_none()
        .size(px(8.))
        .rounded_full()
        .bg(status_color(status))
}

impl RootView {
    pub(crate) fn render_model_settings(
        &mut self,
        settings: &crate::shared::settings::AppSettings,
        blocked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = active_theme();
        let phone = self.state.read(cx).phone_client.enabled;
        let state = &self.state.read(cx).provider_settings;
        let view = state.view.clone();
        let loading = state.loading;
        let busy = state.busy;
        let unavailable = state.unavailable;
        let error = state.error.clone();
        let adding = state.adding;
        let selected = state.selected_provider().cloned();

        let refresh = IconButton::new("provider-refresh", IconName::RefreshCw)
            .size(ControlSize::Md)
            .variant(ButtonVariant::Outline)
            .tooltip(label("Refresh", "刷新"))
            .disabled(loading || busy || unavailable)
            .on_click(cx.listener(|this, _, _, cx| {
                this.state
                    .update(cx, |s, cx| s.read_provider_settings(true, cx))
            }));
        let add = Button::new("provider-add", label("Add provider", "添加提供商"))
            .icon(IconName::Plus)
            .size(ControlSize::Md)
            .disabled(view.is_none() || busy)
            .on_click(cx.listener(|this, _, _, cx| {
                this.state.update(cx, |s, cx| {
                    s.provider_settings.adding = true;
                    s.provider_settings.confirm_delete = None;
                    cx.notify();
                })
            }));
        let actions = div()
            .flex()
            .items_center()
            .gap_2()
            .child(refresh)
            .child(add);
        #[cfg(test)]
        let actions = crate::app::test_support::track_children(
            actions,
            vec!["provider-refresh".into(), "provider-add".into()],
        );
        let header = div()
            .flex()
            .items_center()
            .gap_4()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(font_size_sm()))
                    .text_color(theme.subtle)
                    .child(label(
                        "Configure model providers, API keys and models. Changes apply to every workspace.",
                        "配置模型提供商、API 密钥和模型。修改对所有工作区生效。",
                    )),
            )
            .when(!unavailable, |el| el.child(actions));
        let mut page = div().flex().flex_col().gap_4().child(header);
        if let Some(error) = error {
            page = page.child(
                div()
                    .text_size(px(font_size_sm()))
                    .text_color(theme.destructive)
                    .child(error),
            );
        }
        let Some(view) = view else {
            let note = if unavailable {
                label(
                    "This agent cannot edit providers yet; update GCode to manage them here. Showing the read-only catalog.",
                    "当前智能体尚不支持编辑提供商；请更新 GCode。以下为只读模型目录。",
                )
            } else {
                label("Loading providers…", "正在加载提供商…")
            };
            let note = div()
                .text_size(px(font_size_sm()))
                .text_color(theme.subtle)
                .child(note);
            return page
                .when(loading || unavailable, |el| el.child(note))
                .child(self.render_model_catalog(window, cx))
                .into_any_element();
        };
        self.sync_provider_form(selected.as_ref(), view.revision, window, cx);
        self.sync_auto_compact_inputs(selected.as_ref(), window, cx);
        let detail = if adding {
            self.render_provider_add(&view, busy, cx)
        } else if let Some(provider) = selected.as_ref() {
            // 缓存生命周期按 Provider 存；只有 Anthropic Messages 格式会发送 cache marker。
            // 手机端没有自己的智能体，不显示。
            let prompt_cache = (!phone && provider.api_type == "anthropic-messages").then(|| {
                crate::app::settings_prompt_cache::prompt_cache_control(
                    &provider.id,
                    settings,
                    blocked,
                )
            });
            self.render_provider_card(provider, busy, prompt_cache, cx)
        } else {
            div()
                .text_color(theme.subtle)
                .child(label("No providers configured yet.", "尚未配置提供商。"))
                .into_any_element()
        };
        let selected_id = (!adding).then(|| selected.map(|p| p.id)).flatten();
        let card = div()
            .w_full()
            .min_h(px(CARD_MIN_HEIGHT))
            .flex()
            .rounded_xl()
            .border_1()
            .border_color(theme.border)
            .bg(theme.card)
            .overflow_hidden()
            .child(self.render_provider_nav(&view, selected_id.as_deref(), busy, cx))
            .child(div().flex_1().min_w_0().p_5().child(detail));
        page.child(card).into_any_element()
    }

    fn render_provider_nav(
        &mut self,
        view: &ProviderView,
        selected: Option<&str>,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = active_theme();
        let mut nav = div()
            .w(px(NAV_WIDTH))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .border_r_1()
            .border_color(theme.border);
        for (title, providers) in [
            (label("Preset", "预置"), view.presets()),
            (label("Custom", "自定义"), view.customs()),
        ] {
            if providers.is_empty() {
                continue;
            }
            nav = nav.child(
                div()
                    .px_2()
                    .pt_2()
                    .pb_1()
                    .text_size(px(font_size_sm()))
                    .text_color(theme.subtle)
                    .child(title),
            );
            for provider in providers {
                let active = selected == Some(provider.id.as_str());
                nav = nav.child(self.provider_nav_row(view, provider, active, busy, cx));
            }
        }
        nav.into_any_element()
    }

    fn provider_nav_row(
        &mut self,
        view: &ProviderView,
        provider: &Provider,
        active: bool,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = active_theme();
        let id = provider.id.clone();
        let mut row = div()
            .id(SharedString::from(format!("provider-nav-{}", provider.id)))
            .h(px(32.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded_lg()
            .cursor_pointer()
            .text_size(px(ui_size(14.)))
            .text_color(theme.foreground)
            .when(active, |el| el.bg(theme.surface_hover))
            .when(!active, |el| el.hover(|s| s.bg(theme.surface_hover)))
            .child(status_dot(provider.status()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(provider.name.clone()),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                let id = id.clone();
                this.state.update(cx, |s, cx| {
                    let page = &mut s.provider_settings;
                    page.selected = Some(id);
                    page.adding = false;
                    page.confirm_delete = None;
                    cx.notify();
                })
            }));
        if active && provider.custom() {
            for (up, icon) in [(true, IconName::ChevronUp), (false, IconName::ChevronDown)] {
                let order = view.moved_custom_order(&provider.id, up);
                let target = format!("provider-nav-{}", if up { "up" } else { "down" });
                row = row.child(
                    IconButton::new(SharedString::from(target), icon)
                        .size(ControlSize::Sm)
                        .tooltip(if up {
                            label("Move up", "上移")
                        } else {
                            label("Move down", "下移")
                        })
                        .disabled(busy || order.is_none())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(order) = order.clone() {
                                let operation =
                                    serde_json::json!({ "type": "reorderProviders", "providerIds": order });
                                this.mutate_provider(operation, cx);
                            }
                        })),
                );
            }
        }
        let row = div().child(row);
        #[cfg(test)]
        let row = crate::app::test_support::track_children(
            row,
            vec![format!("provider-nav-{}", provider.id)],
        );
        row.into_any_element()
    }
}
