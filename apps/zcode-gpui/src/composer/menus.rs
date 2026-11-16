//! Composer pickers built on ely-gpui-component menus: model (provider-grouped),
//! thinking level and collaboration mode. Desktop parity:
//! packages/ui/src/v4/composer/V4ComposerModeControls.tsx + ModelConfigSelect.
//! The context-window ring leads the row, as in Desktop's model controls.
//!
//! Ely hosts render a `Ghost` trigger button and float the panel under it,
//! flipping above when the composer sits at the window's bottom edge
//! (ely `float`/`opens_up`), so the old hand-rolled bottom_full popup is gone.

use crate::app::root::RootView;
use crate::composer::catalog::{ModelOption, group_models};
use crate::composer::config_cmds::{EffectiveConfig, title_case};
use crate::composer::plan_mode::{PERMISSION_MODES, mode_label};
use ely_gpui_component::buttons::{Button, ButtonVariant};
use ely_gpui_component::menus::{DropdownMenu, Menu, MenuItem};
use ely_gpui_component::primitives::{Icon, IconName, Tooltip};
use gpui::{Context, Entity, IntoElement, ParentElement, Styled, div, prelude::*};

impl RootView {
    /// Model + thinking-level pickers for the composer's bottom row.
    pub(crate) fn composer_selectors(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let effective = self.state.read(cx).effective_config();
        let models: Vec<ModelOption> = self
            .state
            .read(cx)
            .active_ws_key()
            .and_then(|k| self.state.read(cx).workspace_configs.get(&k))
            .map(|c| c.models.clone())
            .unwrap_or_default();
        let root = cx.entity();
        // 手机宽度下整行放不下：去掉提供商前缀并隐藏 tok/s，否则末尾溢出出卡片、发送按钮被挤到下一行。
        let compact = self.composer_compact;
        let (rate, context_window) = {
            let state = self.state.read(cx);
            let conversation = state
                .active
                .as_ref()
                .and_then(|sid| state.conversations.get(sid));
            (
                conversation
                    .filter(|_| !compact)
                    .and_then(|c| c.throughput.rate(std::time::Instant::now())),
                conversation.and_then(|c| c.context_window.clone()),
            )
        };
        #[cfg(test)]
        let tracked: Vec<String> = [
            context_window.is_some().then_some("context-ring"),
            (!models.is_empty()).then_some("model-menu-row"),
            (!effective.thought_levels.is_empty()).then_some("thought-menu-row"),
            rate.is_some().then_some("tokens-per-second"),
        ]
        .into_iter()
        .flatten()
        .map(str::to_string)
        .collect();

        let row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .justify_end()
            .min_w_0()
            .items_center()
            .gap_1()
            .children(context_window.map(crate::composer::context_ring::context_ring))
            .children(
                effective
                    .model_unavailable(&models)
                    .then(model_unavailable_marker),
            )
            .children((!models.is_empty()).then(|| {
                DropdownMenu::new(
                    "model-menu",
                    match models
                        .iter()
                        .find(|o| o.provider == effective.provider && o.model == effective.model)
                    {
                        Some(opt) if compact => opt.name.clone(),
                        _ => effective.trigger_display(&models),
                    },
                    model_menu(&effective, &models, root.clone()),
                )
                .variant(ButtonVariant::Ghost)
                .into_any_element()
            }))
            .children((!effective.thought_levels.is_empty()).then(|| {
                let trigger = div().flex().items_center().child(
                    DropdownMenu::new(
                        "thought-menu",
                        effective.thought_display(),
                        thought_menu(&effective, root.clone()),
                    )
                    .variant(ButtonVariant::Ghost),
                );
                #[cfg(test)]
                let trigger =
                    crate::app::test_support::track_children(trigger, vec!["thought-menu".into()]);
                trigger.into_any_element()
            }))
            .children(rate.map(|r| {
                div()
                    .id("tokens-per-second")
                    .flex_none()
                    .text_size(gpui::px(crate::shared::theme::ui_size(12.)))
                    .text_color(crate::shared::theme_colors::color(
                        crate::shared::theme::MUTED,
                    ))
                    .child(crate::conversation::throughput::format_rate(r))
            }));
        #[cfg(test)]
        let row = crate::app::test_support::track_children(row, tracked);
        row
    }

    /// Mode picker (left of the composer row): the trigger names the
    /// permission mode; an active plan overlay adds a "Plan" chip that turns
    /// it off (Desktop `v4-composer-plan-marker`).
    pub(crate) fn mode_menu(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let effective: EffectiveConfig = self.state.read(cx).effective_config();
        let root = cx.entity();
        let chip_root = root.clone();
        let row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .child(
                DropdownMenu::new(
                    "mode-menu",
                    mode_label(&effective.mode),
                    mode_menu(&effective, root),
                )
                .icon(IconName::Shield)
                .variant(ButtonVariant::Ghost),
            )
            .children(effective.plan_enabled.then(|| {
                Button::new("plan-marker", crate::shared::i18n::t("mode.plan"))
                    .icon(IconName::Lightbulb)
                    .variant(ButtonVariant::Ghost)
                    .on_click(move |_, _, cx| {
                        chip_root.update(cx, |root, cx| {
                            root.state.update(cx, |s, cx| s.set_plan_enabled(false, cx));
                        });
                    })
            }));
        #[cfg(test)]
        let row = crate::app::test_support::track_children(
            row,
            [
                Some("mode-menu"),
                effective.plan_enabled.then_some("plan-marker"),
            ]
            .into_iter()
            .flatten()
            .map(str::to_string)
            .collect(),
        );
        row
    }
}

/// Warning glyph beside the model trigger when the session's model left the
/// catalog; the tooltip says what to do (pick another model).
fn model_unavailable_marker() -> gpui::AnyElement {
    let tip = crate::shared::i18n::label(
        "This session's model is no longer available. Pick another model.",
        "此会话的模型已不可用，请选择其他模型。",
    );
    div()
        .id("model-unavailable")
        .flex_none()
        .flex()
        .items_center()
        .text_color(crate::shared::theme_colors::color(
            crate::shared::theme::WARNING,
        ))
        .tooltip(Tooltip::text(tip))
        .child(Icon::new(IconName::TriangleAlert))
        .into_any_element()
}

/// Provider-grouped model menu (desktop parity: registry groups render as
/// titled sections). Rows carry the RAW option value semantics the draft
/// config lookup (`find_model_option`) matches on.
fn model_menu(effective: &EffectiveConfig, models: &[ModelOption], root: Entity<RootView>) -> Menu {
    let mut menu = Menu::new();
    for group in group_models(models) {
        let items = group.items.iter().map(|opt| {
            let selected = effective.provider == opt.provider && effective.model == opt.model;
            let value_provider = opt.provider.clone();
            let value_model = opt.model.clone();
            // Raw option value (may carry a `$thought` suffix) so draft-config
            // lookup (`find_model_option`) matches exactly.
            let raw_value = opt.value.clone();
            let thought = if opt.thought_levels.is_empty() {
                String::new()
            } else {
                opt.default_thought.clone()
            };
            let root = root.clone();
            MenuItem::radio(opt.name.clone(), selected).on_click(move |_, cx| {
                root.update(cx, |root, cx| {
                    root.state.update(cx, |s, cx| {
                        if s.set_restored_model(&value_provider, &value_model, &thought) {
                            cx.notify();
                        } else if s.effective_config().has_session {
                            s.switch_model(&value_provider, &value_model, &thought, cx);
                        } else {
                            s.ui_model_value = Some(raw_value.clone());
                            s.ui_thought = None;
                        }
                    });
                });
            })
        });
        menu = menu.group(group.label, items);
    }
    menu
}

/// Thinking-level menu: "default" plus the current model's levels.
fn thought_menu(effective: &EffectiveConfig, root: Entity<RootView>) -> Menu {
    let current = effective.thought.clone();
    let (provider, model) = (effective.provider.clone(), effective.model.clone());
    let mut levels = vec![String::new()];
    levels.extend(effective.thought_levels.iter().cloned());
    let root = root.clone();

    let mut menu = Menu::new();
    for level in levels {
        let selected = if level.is_empty() {
            current.is_empty()
        } else {
            level == current
        };
        let label = if level.is_empty() {
            "Default".to_string()
        } else {
            title_case(&level)
        };
        let (provider, model) = (provider.clone(), model.clone());
        let lv = level.clone();
        let root = root.clone();
        menu = menu.item(MenuItem::radio(label, selected).on_click(move |_, cx| {
            root.update(cx, |root, cx| {
                root.state.update(cx, |s, cx| {
                    crate::composer::thought_cycle::apply_thought(s, &provider, &model, &lv, cx);
                });
            });
        }));
    }
    menu
}

/// Desktop `V4ComposerModeSwitch`: a Plan check row, then the permission
/// radios. Legacy Edit is listed only while it is the current mode.
fn mode_menu(effective: &EffectiveConfig, root: Entity<RootView>) -> Menu {
    let plan_on = effective.plan_enabled;
    let plan_root = root.clone();
    let mut menu = Menu::new()
        .item(
            MenuItem::check(crate::shared::i18n::t("mode.label.glm.plan"), plan_on)
                .icon(IconName::Lightbulb)
                .on_click(move |_, cx| {
                    plan_root.update(cx, |root, cx| {
                        root.state
                            .update(cx, |s, cx| s.set_plan_enabled(!plan_on, cx));
                    });
                }),
        )
        .separator();
    let legacy_edit = (effective.mode == "edit").then_some("edit");
    for id in PERMISSION_MODES.into_iter().chain(legacy_edit) {
        let root = root.clone();
        menu = menu.item(
            MenuItem::radio(mode_label(id), effective.mode == id).on_click(move |_, cx| {
                root.update(cx, |root, cx| {
                    root.state
                        .update(cx, |s, cx| s.select_permission_mode(id, cx));
                });
            }),
        );
    }
    menu
}
