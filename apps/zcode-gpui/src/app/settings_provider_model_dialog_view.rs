//! Add-model dialog rendering (desktop `ProviderModelMetadataDialog`, add
//! mode): smart switch, ID, token limits and the collapsible advanced part.

use crate::app::root::RootView;
use crate::app::settings_provider_model_dialog::{AddModelDialog, DialogError};
use crate::provider_settings::model_draft::{CAPABILITIES, DraftField, MODALITIES};
use crate::shared::i18n::label;
use crate::shared::theme::{MONO_FONT, active_theme, font_size_sm, font_size_xs};
use ely_gpui_component::buttons::{Button, ButtonVariant, IconButton};
use ely_gpui_component::forms::{Input, Switch, TextInput};
use ely_gpui_component::overlays::Dialog;
use ely_gpui_component::primitives::{Icon, IconName};
use ely_gpui_component::theme::ControlSize;
use gpui::{AnyElement, ClickEvent, Context, Entity, IntoElement, div, prelude::*, px};

fn caption(text: &str) -> gpui::Div {
    div()
        .text_size(px(font_size_sm()))
        .text_color(active_theme().subtle)
        .child(text.to_owned())
}

fn field(title: &str, input: AnyElement) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(caption(title))
        .child(input)
        .into_any_element()
}

fn text_input(input: &Entity<TextInput>, mono: bool, invalid: bool) -> AnyElement {
    div()
        .when(mono, |el| el.font_family(MONO_FONT))
        .child(Input::new(input).invalid(invalid))
        .into_any_element()
}

fn fails(dialog: &AddModelDialog, fields: &[DraftField]) -> bool {
    matches!(&dialog.error, Some(DialogError::Field(f)) if fields.contains(f))
}

impl RootView {
    pub(crate) fn render_add_model_dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.add_model_dialog()?;
        let theme = active_theme();
        let shown = dialog.shown();
        let saving = dialog.saving();
        let weak = cx.entity().downgrade();
        let on_close = {
            let weak = weak.clone();
            move |_: &mut gpui::Window, cx: &mut gpui::App| {
                let _ = weak.update(cx, |this, cx| this.close_add_model_dialog(cx));
            }
        };
        let smart = Switch::new("provider-model-dialog-smart", shown.recommended)
            .label(label("Smart configuration", "智能配置"))
            .disabled(saving)
            .on_change({
                let weak = weak.clone();
                move |on, _, cx| {
                    let _ = weak.update(cx, |this, cx| this.set_model_dialog_recommended(on, cx));
                }
            });
        let id_invalid =
            fails(dialog, &[DraftField::Id]) || dialog.error == Some(DialogError::Duplicate);
        let mut max_caption = div()
            .flex()
            .items_center()
            .gap_2()
            .child(caption(label("Max output tokens", "最大输出 Token")));
        if dialog.pending() && dialog.draft.recommended {
            max_caption = max_caption.child(
                div()
                    .text_size(px(font_size_xs()))
                    .text_color(theme.subtlest)
                    .child(label("Loading…", "加载中…")),
            );
        }
        let max_output = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(max_caption)
            .child(text_input(
                &dialog.max_output,
                false,
                fails(dialog, &[DraftField::MaxOutputTokens]),
            ));
        let advanced_toggle = Button::new(
            "provider-model-dialog-advanced",
            label("Advanced settings", "高级设置"),
        )
        .icon(if dialog.advanced {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        })
        .variant(ButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_model_dialog_advanced(cx)));
        let mut body = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(smart)
            .child(field(
                label("Model ID", "模型 ID"),
                text_input(&dialog.id, true, id_invalid),
            ))
            .child(field(
                label("Context window", "上下文窗口"),
                text_input(
                    &dialog.context_window,
                    false,
                    fails(dialog, &[DraftField::ContextWindow]),
                ),
            ))
            .child(max_output)
            .child(div().child(advanced_toggle));
        if dialog.advanced {
            body = body.child(self.render_model_dialog_advanced(cx));
        }
        let dialog = self.add_model_dialog()?;
        let feedback = match &dialog.error {
            Some(error) => Some((error.message(), theme.destructive)),
            None if dialog.draft.recommended
                && !dialog.draft.id.trim().is_empty()
                && !dialog.pending() =>
            {
                Some((
                    label(
                        "Smart configuration matched for this model",
                        "已匹配到智能配置",
                    )
                    .to_owned(),
                    theme.success,
                ))
            }
            None => None,
        };
        if let Some((text, color)) = feedback {
            body = body.child(
                div()
                    .text_size(px(font_size_sm()))
                    .text_color(color)
                    .child(text),
            );
        }
        let save = cx.listener(|this, _: &ClickEvent, _, cx| this.save_add_model_dialog(cx));
        let cancel = cx.listener(|this, _: &ClickEvent, _, cx| this.close_add_model_dialog(cx));
        #[cfg(test)]
        let body = crate::app::test_support::track_children(
            div().child(body),
            vec!["provider-model-dialog-body".into()],
        );
        let focus_input = match &dialog.editing {
            Some(_) => &dialog.context_window,
            None => &dialog.id,
        };
        let first_focus = gpui::Focusable::focus_handle(focus_input.read(cx), cx);
        let title = if dialog.editing.is_some() {
            label("Edit model", "编辑模型")
        } else {
            label("Add model", "添加模型")
        };
        let element = Dialog::new(
            gpui::ElementId::Name(format!("provider-model-dialog-{}", dialog.instance).into()),
            title,
            on_close,
        )
        .focus_first(first_focus)
        .child(body)
        .action(move |_| {
            let button = Button::new("provider-model-dialog-cancel", label("Cancel", "取消"))
                .variant(ButtonVariant::Secondary)
                .disabled(saving)
                .on_click(cancel);
            #[cfg(test)]
            let button = crate::app::test_support::track_children(
                div().child(button),
                vec!["provider-model-dialog-cancel".into()],
            );
            button
        })
        .action(move |_| {
            let button = Button::new("provider-model-dialog-save", label("Save", "保存"))
                .variant(ButtonVariant::Primary)
                .loading(saving)
                .disabled(saving)
                .on_click(save);
            #[cfg(test)]
            let button = crate::app::test_support::track_children(
                div().child(button),
                vec!["provider-model-dialog-save".into()],
            );
            button
        });
        Some(element.into_any_element())
    }

    fn render_model_dialog_advanced(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(dialog) = self.add_model_dialog() else {
            return div().into_any_element();
        };
        let shown = dialog.shown();
        let chip = |id: String, text: &str, on: bool| {
            Button::new(id, text.to_owned())
                .size(ControlSize::Sm)
                .variant(if on {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Outline
                })
        };
        let mut modalities = div().flex().flex_wrap().gap_2().child(
            chip(
                "provider-model-dialog-input-text".into(),
                label("Text", "文本"),
                true,
            )
            .disabled(true),
        );
        for key in MODALITIES {
            let text = match key {
                "supportsImage" => label("Image", "图片"),
                "supportsVideo" => label("Video", "视频"),
                _ => "PDF",
            };
            modalities = modalities.child(
                chip(
                    format!("provider-model-dialog-input-{key}"),
                    text,
                    shown.input_flag(key),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.toggle_model_dialog_input(key, cx)
                })),
            );
        }
        let mut capabilities = div().flex().flex_wrap().gap_2();
        for (index, key) in CAPABILITIES.iter().enumerate() {
            let text = match index {
                0 => label("Structured output", "结构化输出"),
                1 => label("Native web search", "原生联网搜索"),
                _ => label("Mid-conversation system messages", "对话中系统消息"),
            };
            capabilities = capabilities.child(
                chip(
                    format!("provider-model-dialog-{key}"),
                    text,
                    shown.capabilities[index],
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.toggle_model_dialog_capability(index, cx)
                })),
            );
        }
        let theme = active_theme();
        let mut levels = div().flex().flex_wrap().items_center().gap_2();
        for (index, level) in shown.levels.iter().enumerate() {
            levels = levels.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .pl_2()
                    .rounded_md()
                    .bg(theme.tag)
                    .font_family(MONO_FONT)
                    .text_size(px(font_size_sm()))
                    .text_color(theme.foreground)
                    .child(level.clone())
                    .child(
                        IconButton::new(
                            format!("provider-model-dialog-level-{index}"),
                            IconName::X,
                        )
                        .size(ControlSize::Sm)
                        .tooltip(label("Delete reasoning level", "删除推理等级"))
                        .on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| this.remove_model_level(index, cx),
                        )),
                    ),
            );
        }
        let adder = div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .w(px(140.))
                    .child(text_input(&dialog.new_level, true, false)),
            )
            .child(
                IconButton::new("provider-model-dialog-level-add", IconName::Plus)
                    .size(ControlSize::Sm)
                    .tooltip(label("Add reasoning level", "添加推理等级"))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.add_model_level(cx))),
            );
        let levels_invalid = fails(dialog, &[DraftField::ReasoningLevelValues]);
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(field(
                label("Input types", "输入类型"),
                modalities.into_any_element(),
            ))
            .child(field(
                label("Model capabilities", "模型能力"),
                capabilities.into_any_element(),
            ))
            .child(field(
                label("Reasoning levels (low to high)", "推理等级（从低到高）"),
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(levels.child(adder))
                    .when(levels_invalid, |el| {
                        el.child(
                            div()
                                .text_color(theme.destructive)
                                .child(Icon::new(IconName::CircleAlert)),
                        )
                    })
                    .into_any_element(),
            ))
            .child(field(
                label("Reasoning parameter mapping", "推理参数映射"),
                text_input(
                    &dialog.level_map,
                    true,
                    fails(dialog, &[DraftField::ReasoningLevelMap]),
                ),
            ))
            .child(field(
                label("Max output parameter mapping", "最大输出参数映射"),
                text_input(&dialog.max_output_map, true, false),
            ))
            .into_any_element()
    }
}
