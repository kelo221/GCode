//! Model settings: the provider's model list (desktop `ProviderModelList`) —
//! add through the add-model dialog, test connectivity, reorder, enable and
//! delete custom models.

use crate::app::root::RootView;
use crate::provider_settings::{Model, Provider, TestStatus, compact_tokens, moved};
use crate::shared::i18n::label;
use crate::shared::theme::{MONO_FONT, active_theme, font_size_sm, font_size_xs, ui_size};
use ely_gpui_component::buttons::{Button, ButtonVariant, IconButton};
use ely_gpui_component::forms::Switch;
use ely_gpui_component::primitives::IconName;
use ely_gpui_component::theme::ControlSize;
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};
use serde_json::json;

fn badge(text: String) -> AnyElement {
    let theme = active_theme();
    div()
        .flex_none()
        .px_1p5()
        .rounded_md()
        .bg(theme.tag)
        .text_size(px(font_size_xs()))
        .text_color(theme.subtle)
        .child(text)
        .into_any_element()
}

impl RootView {
    pub(crate) fn render_provider_models(
        &mut self,
        provider: &Provider,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = active_theme();
        let mut list = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(self.provider_model_adder(provider, busy, cx));
        if provider.models.is_empty() {
            return list
                .child(
                    div()
                        .text_size(px(font_size_sm()))
                        .text_color(theme.subtle)
                        .child(label(
                            "No models yet. Add one by its model ID.",
                            "暂无模型。请通过模型 ID 添加。",
                        )),
                )
                .into_any_element();
        }
        let ids: Vec<String> = provider.models.iter().map(|m| m.id.clone()).collect();
        let mut rows = div()
            .flex()
            .flex_col()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .overflow_hidden();
        for (index, model) in provider.models.iter().enumerate() {
            rows = rows.child(
                div()
                    .when(index > 0, |el| el.border_t_1().border_color(theme.border))
                    .child(self.provider_model_row(provider, model, &ids, busy, cx)),
            );
        }
        list = list.child(rows);
        list.into_any_element()
    }

    fn provider_model_adder(
        &mut self,
        provider: &Provider,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = active_theme();
        let title = div()
            .flex_1()
            .text_size(px(ui_size(14.)))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(theme.foreground)
            .child(format!(
                "{} ({})",
                label("Models", "模型"),
                provider.models.len()
            ));
        // 旧的行内输入在空/重复 ID 时点击“添加”静默无反应；改为与 Desktop 一致的新增模型弹窗，
        // 校验错误与 agent 错误都在弹窗内展示。
        let provider_id = provider.id.clone();
        let adder = div().child(
            Button::new("provider-model-add", label("Add model", "添加模型"))
                .icon(IconName::Plus)
                .size(ControlSize::Md)
                .variant(ButtonVariant::Secondary)
                .disabled(busy)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_add_model_dialog(&provider_id, window, cx)
                })),
        );
        #[cfg(test)]
        let adder =
            crate::app::test_support::track_children(adder, vec!["provider-model-add".into()]);
        let dialog = self
            .add_model_dialog()
            .is_some_and(|d| d.provider == provider.id)
            .then(|| self.render_add_model_dialog(cx))
            .flatten();
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(title)
            .child(adder)
            .children(dialog)
            .into_any_element()
    }

    fn provider_model_row(
        &mut self,
        provider: &Provider,
        model: &Model,
        ids: &[String],
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = active_theme();
        let key = (provider.id.clone(), model.id.clone());
        let test = self
            .state
            .read(cx)
            .provider_settings
            .tests
            .get(&key)
            .cloned();
        let slug = model.id.replace(['/', ':', '.'], "-");

        let mut info = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_family(MONO_FONT)
                    .text_size(px(ui_size(13.)))
                    .text_color(if model.enabled {
                        theme.foreground
                    } else {
                        theme.subtlest
                    })
                    .child(model.id.clone()),
            );
        if let Some(tokens) = model.context_window {
            info = info.child(badge(compact_tokens(tokens)));
        }
        if model.vision {
            info = info.child(badge(label("Vision", "视觉").to_owned()));
        }
        if model.builtin {
            info = info.child(badge(label("Built-in", "内置").to_owned()));
        }

        let status = match &test {
            Some(TestStatus::Passed) => Some((IconName::CircleCheck, theme.success, None)),
            Some(TestStatus::Failed(message)) => {
                Some((IconName::CircleX, theme.destructive, Some(message.clone())))
            }
            _ => None,
        };
        let mut actions = div().flex().items_center().gap_1();
        if let Some((icon, color, _)) = &status {
            actions = actions.child(
                div()
                    .text_color(*color)
                    .child(ely_gpui_component::primitives::Icon::new(*icon)),
            );
        }
        let (pid, mid) = key.clone();
        actions = actions.child(
            Button::new(format!("provider-model-test-{slug}"), label("Test", "测试"))
                .size(ControlSize::Sm)
                .variant(ButtonVariant::Ghost)
                .loading(test == Some(TestStatus::Running))
                .disabled(test == Some(TestStatus::Running) || !provider.enabled)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let (pid, mid) = (pid.clone(), mid.clone());
                    this.state
                        .update(cx, |s, cx| s.test_provider_model(&pid, &mid, cx));
                })),
        );
        for (up, icon) in [(true, IconName::ChevronUp), (false, IconName::ChevronDown)] {
            let mut order = ids.to_vec();
            let can_move = moved(&mut order, &model.id, up);
            let pid = provider.id.clone();
            actions = actions.child(
                IconButton::new(
                    format!("provider-model-{}-{slug}", if up { "up" } else { "down" }),
                    icon,
                )
                .size(ControlSize::Sm)
                .tooltip(if up {
                    label("Move up", "上移")
                } else {
                    label("Move down", "下移")
                })
                .disabled(busy || !can_move)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let operation =
                        json!({ "type": "reorderModels", "providerId": pid, "modelIds": order });
                    this.mutate_provider(operation, cx);
                })),
            );
        }
        // 模型配置原先只在添加时可见；这里重开同一弹窗（编辑模式），对应 Desktop 的模型编辑入口。
        let (pid, edited) = (provider.id.clone(), model.clone());
        let edit = div().child(
            IconButton::new(format!("provider-model-edit-{slug}"), IconName::Settings)
                .size(ControlSize::Sm)
                .tooltip(label("Model settings", "模型设置"))
                .disabled(busy)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_edit_model_dialog(&pid, &edited, window, cx)
                })),
        );
        #[cfg(test)]
        let edit = crate::app::test_support::track_children(
            edit,
            vec![format!("provider-model-edit-{slug}")],
        );
        actions = actions.child(edit);
        if !model.builtin {
            let (pid, mid) = key.clone();
            actions = actions.child(
                IconButton::new(format!("provider-model-delete-{slug}"), IconName::Trash2)
                    .size(ControlSize::Sm)
                    .tooltip(label("Delete model", "删除模型"))
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let operation =
                            json!({ "type": "deleteModel", "providerId": pid, "modelId": mid });
                        this.mutate_provider(operation, cx);
                    })),
            );
        }
        actions =
            actions.children(self.auto_compact_model_control(&provider.id, &model.id, &slug, cx));
        let (pid, mid) = key;
        let weak = cx.entity().downgrade();
        actions = actions.child(
            Switch::new(format!("provider-model-enabled-{slug}"), model.enabled)
                .disabled(busy)
                .on_change(move |enabled, _, cx| {
                    let operation = json!({
                        "type": "setModelEnabled",
                        "providerId": pid,
                        "modelId": mid,
                        "enabled": enabled,
                    });
                    let _ = weak.update(cx, |this, cx| this.mutate_provider(operation, cx));
                }),
        );

        let mut notes: Vec<String> = model.issues.clone();
        if let Some((_, _, Some(message))) = status {
            notes.push(message);
        }
        let row = div()
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(info)
                    .child(actions),
            )
            .children(notes.into_iter().map(|note| {
                div()
                    .text_size(px(font_size_xs()))
                    .text_color(theme.warning)
                    .child(note)
            }));
        #[cfg(test)]
        let row = crate::app::test_support::track_children(
            div().child(row),
            vec![format!("provider-model-{slug}")],
        );
        row.into_any_element()
    }
}
