//! Model settings detail pane: desktop `InlineEditableProviderCard` —
//! header with enable/delete, then name, endpoint, API format and key rows.

use crate::app::root::RootView;
use crate::app::settings_models::status_dot;
use crate::app::settings_provider_form::Field;
use crate::provider_settings::{API_FORMATS, Provider};
use crate::shared::i18n::label;
use crate::shared::theme::{MONO_FONT, active_theme, font_size_sm, ui_size};
use ely_gpui_component::buttons::{Button, ButtonVariant, IconButton};
use ely_gpui_component::forms::{Choice, Input, Select, Switch};
use ely_gpui_component::primitives::IconName;
use ely_gpui_component::theme::ControlSize;
use gpui::{AnyElement, Context, IntoElement, SharedString, div, prelude::*, px};
use serde_json::{Value, json};

const LABEL_WIDTH: f32 = 120.;

fn api_format_label(format: &str) -> &'static str {
    match format {
        "anthropic-messages" => "Anthropic Messages",
        "openai-chat-completions" => "OpenAI Chat Completions",
        "openai-responses" => "OpenAI Responses",
        _ => "Custom",
    }
}

/// One labelled line of the card; `control` fills the remaining width.
fn field_row(title: &str, control: impl IntoElement) -> AnyElement {
    let theme = active_theme();
    div()
        .flex()
        .items_center()
        .gap_3()
        .min_h(px(36.))
        .child(
            div()
                .w(px(LABEL_WIDTH))
                .flex_none()
                .text_size(px(ui_size(14.)))
                .text_color(theme.subtle)
                .child(title.to_owned()),
        )
        .child(div().flex_1().min_w_0().child(control))
        .into_any_element()
}

fn read_only(text: &str) -> AnyElement {
    let theme = active_theme();
    let shown = if text.is_empty() { "—" } else { text };
    div()
        .text_size(px(ui_size(14.)))
        .text_color(theme.foreground)
        .truncate()
        .child(shown.to_owned())
        .into_any_element()
}

impl RootView {
    pub(crate) fn mutate_provider(&mut self, operation: Value, cx: &mut Context<Self>) {
        self.state
            .update(cx, |s, cx| s.mutate_provider_settings(operation, cx));
    }

    pub(crate) fn render_provider_card(
        &mut self,
        provider: &Provider,
        busy: bool,
        prompt_cache: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = active_theme();
        let mut card = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(self.provider_card_header(provider, busy, cx));
        let mut rows = div().flex().flex_col().gap_2();
        rows = rows.child(field_row(
            label("Name", "名称"),
            self.provider_text_field(Field::Name, provider.custom(), &provider.name, false),
        ));
        rows = rows.child(field_row(
            label("Base URL", "接口地址"),
            self.provider_text_field(Field::BaseUrl, provider.custom(), &provider.base_url, true),
        ));
        rows = rows.child(field_row(
            label("API format", "接口格式"),
            self.provider_format_field(provider, busy, cx),
        ));
        if provider.uses_api_key() {
            rows = rows.child(field_row(
                label("API key", "API 密钥"),
                self.provider_key_field(cx),
            ));
        } else if provider.account() {
            rows = rows.child(field_row(
                label("Access", "访问方式"),
                read_only(label(
                    "Signed-in account (manage it in the desktop app)",
                    "账号登录（请在桌面版中管理）",
                )),
            ));
        }
        if let Some(control) = prompt_cache {
            rows = rows.child(field_row(label("Prompt cache", "提示词缓存"), control));
        }
        card = card.child(rows);
        if !provider.issues.is_empty() {
            card = card.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .text_size(px(font_size_sm()))
                    .text_color(theme.warning)
                    .children(
                        provider
                            .issues
                            .iter()
                            .map(|issue| div().child(issue.clone())),
                    ),
            );
        }
        card.child(div().border_t_1().border_color(theme.border))
            .child(self.render_provider_models(provider, busy, cx))
            .into_any_element()
    }

    fn provider_card_header(
        &mut self,
        provider: &Provider,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = active_theme();
        let confirming = self
            .state
            .read(cx)
            .provider_settings
            .confirm_delete
            .as_deref()
            == Some(provider.id.as_str());
        let mut actions = div().flex().items_center().gap_2();
        let mut targets = Vec::new();
        if !provider.account() {
            let id = provider.id.clone();
            let weak = cx.entity().downgrade();
            actions = actions.child(
                Switch::new("provider-enabled", provider.enabled)
                    .disabled(busy)
                    .on_change(move |enabled, _, cx| {
                        let operation =
                            json!({ "type": "updateProvider", "providerId": id, "enabled": enabled });
                        let _ = weak.update(cx, |this, cx| this.mutate_provider(operation, cx));
                    }),
            );
            targets.push("provider-enabled".to_owned());
        }
        if provider.custom() {
            if confirming {
                let id = provider.id.clone();
                actions = actions
                    .child(
                        Button::new("provider-delete-confirm", label("Delete", "删除"))
                            .variant(ButtonVariant::Danger)
                            .size(ControlSize::Sm)
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let operation =
                                    json!({ "type": "deleteProvider", "providerId": id });
                                this.mutate_provider(operation, cx);
                            })),
                    )
                    .child(
                        Button::new("provider-delete-cancel", label("Cancel", "取消"))
                            .variant(ButtonVariant::Ghost)
                            .size(ControlSize::Sm)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.state.update(cx, |s, cx| {
                                    s.provider_settings.confirm_delete = None;
                                    cx.notify();
                                })
                            })),
                    );
                targets.extend([
                    "provider-delete-confirm".into(),
                    "provider-delete-cancel".into(),
                ]);
            } else {
                let id = provider.id.clone();
                actions = actions.child(
                    IconButton::new("provider-delete", IconName::Trash2)
                        .tooltip(label("Delete provider", "删除提供商"))
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let id = id.clone();
                            this.state.update(cx, |s, cx| {
                                s.provider_settings.confirm_delete = Some(id);
                                cx.notify();
                            })
                        })),
                );
                targets.push("provider-delete".to_owned());
            }
        }
        #[cfg(test)]
        let actions = crate::app::test_support::track_children(actions, targets);
        #[cfg(not(test))]
        let _ = targets;
        let kind = if provider.custom() {
            label("Custom provider", "自定义提供商")
        } else {
            label("Preset provider", "预置提供商")
        };
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(status_dot(provider.status()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(ui_size(16.)))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .truncate()
                            .child(provider.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(font_size_sm()))
                            .text_color(theme.subtle)
                            .child(kind),
                    ),
            )
            .child(actions)
            .into_any_element()
    }

    fn provider_text_field(
        &self,
        field: Field,
        editable: bool,
        value: &str,
        mono: bool,
    ) -> AnyElement {
        let Some(form) = self.settings.provider_form.as_ref().filter(|_| editable) else {
            let text = read_only(value);
            return if mono {
                div().font_family(MONO_FONT).child(text).into_any_element()
            } else {
                text
            };
        };
        let id = match field {
            Field::Name => "provider-name",
            _ => "provider-base-url",
        };
        let wrapper = div().w_full().child(Input::new(form.input(field)));
        #[cfg(test)]
        let wrapper = crate::app::test_support::track_children(wrapper, vec![id.into()]);
        #[cfg(not(test))]
        let _ = id;
        wrapper.into_any_element()
    }

    fn provider_format_field(
        &mut self,
        provider: &Provider,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !provider.custom() {
            return read_only(api_format_label(&provider.api_type));
        }
        let id = provider.id.clone();
        let current = provider.api_type.clone();
        let select = div().w(px(280.)).child(
            Select::new(
                "provider-api-format",
                API_FORMATS.map(|format| Choice::new(format, api_format_label(format))),
            )
            .selected(SharedString::from(provider.api_type.clone()))
            .disabled(busy)
            .on_change(cx.listener(move |this, value: &SharedString, _, cx| {
                if value.as_ref() == current {
                    return;
                }
                let operation =
                    json!({ "type": "updateProvider", "providerId": id, "apiType": value.as_ref() });
                this.mutate_provider(operation, cx);
            })),
        );
        #[cfg(test)]
        let select =
            crate::app::test_support::track_children(select, vec!["provider-api-format".into()]);
        select.into_any_element()
    }

    fn provider_key_field(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(form) = self.settings.provider_form.as_ref() else {
            return read_only("");
        };
        let (icon, tip) = if form.show_key {
            (IconName::EyeOff, label("Hide key", "隐藏密钥"))
        } else {
            (IconName::Eye, label("Show key", "显示密钥"))
        };
        let row = div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().flex_1().min_w_0().child(Input::new(&form.api_key)))
            .child(
                IconButton::new("provider-key-visibility", icon)
                    .tooltip(tip)
                    .on_click(
                        cx.listener(|this, _, _, cx| this.toggle_provider_key_visibility(cx)),
                    ),
            );
        #[cfg(test)]
        let row = crate::app::test_support::track_children(
            row,
            vec!["provider-api-key".into(), "provider-key-visibility".into()],
        );
        row.into_any_element()
    }
}
