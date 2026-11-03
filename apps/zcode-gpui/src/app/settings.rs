use crate::app::root::RootView;
use crate::app::settings_code_search::code_search_pane;
use crate::app::settings_lsp_diagnostics::lsp_diagnostics_pane;
use crate::app::settings_navigation::settings_layout;
use crate::app::settings_rows::{ControlWidth, settings_card, settings_row};
use crate::shared::i18n::{LocalePreference, t};
use crate::shared::preferences::{PreferenceChange, PreferenceOwner, Preferences};
use crate::shared::theme::{active_theme, font_size_base};
use ely_gpui_component::buttons::Button;
use ely_gpui_component::forms::{Choice, Select, Slider, Switch};
use ely_gpui_component::primitives::IconName;
use gpui::{AnyElement, Context, FocusHandle, IntoElement, Window, div, prelude::*, px};

gpui::actions!(settings, [OpenSettings]);

pub(crate) fn language_choices() -> [Choice; 3] {
    use crate::shared::i18n::label;
    [
        Choice::new("system", label("System", "跟随系统")),
        Choice::new("en-US", label("English", "英语")),
        Choice::new("zh-CN", label("Chinese (Simplified)", "简体中文")),
    ]
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    #[default]
    General,
    Appearance,
    Shortcuts,
    Memory,
    Models,
    Subagents,
    Plugins,
    Mcp,
    Browser,
    Voice,
    SystemPrompt,
    Secrets,
    CodeSearch,
    Diagnostics,
    Usage,
    Phone,
}

impl SettingsSection {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "general" => Self::General,
            "appearance" => Self::Appearance,
            "shortcuts" => Self::Shortcuts,
            "memory" => Self::Memory,
            "models" => Self::Models,
            "subagents" => Self::Subagents,
            "plugins" => Self::Plugins,
            "mcp" => Self::Mcp,
            "browser" => Self::Browser,
            "voice" => Self::Voice,
            "system-prompt" => Self::SystemPrompt,
            "secrets" => Self::Secrets,
            "code-search" => Self::CodeSearch,
            "diagnostics" => Self::Diagnostics,
            "usage" => Self::Usage,
            "phone" => Self::Phone,
            _ => return None,
        })
    }

    /// Panes that pin their own header and scroll an inner region; every other
    /// section scrolls with the whole settings panel.
    pub(crate) fn owns_scroll(self) -> bool {
        matches!(self, Self::Plugins | Self::Usage)
    }
}

pub(crate) struct SettingsView {
    pub open: bool,
    pub section: SettingsSection,
    pub query: String,
    pub shortcut_query: String,
    pub model_query: String,
    pub system_prompt: Option<crate::app::settings_system_prompt::PromptEditor>,
    pub provider_form: Option<crate::app::settings_provider_form::ProviderForm>,
    pub secrets_form: Option<crate::app::settings_secrets::SecretForm>,
    pub focus: FocusHandle,
}

impl SettingsView {
    pub(crate) fn new(focus: FocusHandle) -> Self {
        Self {
            open: false,
            section: Default::default(),
            focus,
            query: String::new(),
            shortcut_query: String::new(),
            model_query: String::new(),
            system_prompt: None,
            provider_form: None,
            secrets_form: None,
        }
    }
}

impl RootView {
    pub(crate) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings_section(self.settings.section, window, cx);
    }

    pub(crate) fn open_settings_section(
        &mut self,
        section: SettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_settings_section(section, cx);
        self.settings.open = true;
        self.quickpick_open = false;
        self.cancel_middle_scroll(cx);
        window.focus(&self.settings.focus, cx);
        cx.notify();
    }

    pub(crate) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.open = false;
        self.subagents.form = None;
        self.state.update(cx, |s, cx| s.close_profiles(cx));
        self.return_focus_to_composer(window, cx);
        cx.notify();
    }

    pub(crate) fn render_settings(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = active_theme();
        let owner = cx.global::<PreferenceOwner>().0.clone();
        let preferences = owner.read(cx);
        let settings = preferences.snapshot.clone();
        let saving = preferences.saving;
        let read_only = preferences.read_only;
        let suspended = preferences.suspended;
        let blocked = saving || read_only || suspended;
        let error = preferences.error.clone();
        let layout = settings_layout(f32::from(window.viewport_size().width));
        let back = div().child(
            crate::app::settings_navigation::nav_item(
                "settings-back",
                IconName::ArrowLeft.path(),
                crate::shared::i18n::label("Back to workspace", "返回工作区"),
                false,
                layout.compact,
            )
            .text_color(theme.subtle)
            .on_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx))),
        );
        #[cfg(test)]
        let back = crate::app::test_support::track_children(back, vec!["settings-back".into()]);
        let navigation = div()
            .id("settings-navigation")
            .w(px(layout.navigation_width))
            .h_full()
            .flex_shrink_0()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_4()
            .px_2()
            .py_3()
            .when(layout.compact, |el| el.items_center())
            .child(back)
            .child(self.render_settings_navigation(layout.compact, window, cx));
        let content = match self.settings.section {
            SettingsSection::General => {
                let selected = settings.language_preference().as_str();
                let selector = div().w_full().child(
                    Select::new("settings-locale", language_choices())
                        .selected(selected)
                        .disabled(blocked)
                        .on_change(|value, _, cx| {
                            Preferences::enqueue(
                                PreferenceChange::Locale(LocalePreference::parse(value)),
                                cx,
                            )
                        }),
                );
                #[cfg(test)]
                let selector = crate::app::test_support::track_children(
                    selector,
                    vec!["settings-locale".into()],
                );
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(settings_card([settings_row(
                        t("settings.locale"),
                        None,
                        selector,
                        ControlWidth::Default,
                    )]))
                    .child(
                        div()
                            .text_color(theme.subtle)
                            .child(crate::shared::i18n::label(
                                "Other settings remain available in GCode desktop.",
                                "其他设置仍可在 GCode 桌面端中使用。",
                            )),
                    )
                    .into_any_element()
            }
            SettingsSection::Appearance => {
                let font_slider = div().w_full().child(
                    Slider::new("settings-font-size", f64::from(font_size_base()))
                        .range(12., 20.)
                        .step(1.)
                        .disabled(blocked)
                        .on_change(|value, _, cx| {
                            Preferences::enqueue(PreferenceChange::FontSize(value as f32), cx)
                        }),
                );
                settings_card([settings_row(
                    crate::shared::i18n::label("Interface font size", "界面字体大小"),
                    Some(format!("{} px", font_size_base()).into()),
                    font_slider,
                    ControlWidth::Wide,
                )])
                .into_any_element()
            }
            SettingsSection::Memory => {
                let switch = div().child(
                    Switch::new(
                        "settings-memory-enabled",
                        settings.memory_enabled.unwrap_or(false),
                    )
                    .disabled(blocked)
                    .on_change(|enabled, _, cx| {
                        Preferences::enqueue(PreferenceChange::MemoryEnabled(enabled), cx)
                    }),
                );
                #[cfg(test)]
                let switch = crate::app::test_support::track_children(
                    switch,
                    vec!["settings-memory-enabled".into()],
                );
                settings_card([settings_row(
                    crate::shared::i18n::label("Enable Memory", "启用记忆"),
                    Some(crate::shared::i18n::label(
                        "Allows configured CLI memory for newly materialized or resumed sessions. This does not reconfigure an active runtime; CLI memory policy still applies.",
                        "允许新建或恢复的会话使用已配置的 CLI 记忆。不会重新配置运行中的会话；仍受 CLI 记忆策略控制。",
                    ).into()),
                    switch,
                    ControlWidth::Default,
                )])
                .into_any_element()
            }
            SettingsSection::Shortcuts => {
                self.render_shortcut_settings(&settings, blocked, window, cx)
            }
            SettingsSection::Models => self.render_model_settings(&settings, blocked, window, cx),
            SettingsSection::Subagents => self.render_subagents_settings(window, cx),
            SettingsSection::Plugins => self.plugins_pane(window, cx),
            SettingsSection::Mcp => self.mcp_pane(&settings, blocked, cx),
            SettingsSection::Browser => self.browser_pane(cx),
            SettingsSection::Voice => self.voice_pane(&settings, blocked, cx),
            SettingsSection::SystemPrompt => self.system_prompt_pane(window, cx),
            SettingsSection::Secrets => self.secrets_pane(window, cx),
            SettingsSection::CodeSearch => code_search_pane(&settings, blocked),
            SettingsSection::Diagnostics => lsp_diagnostics_pane(&settings, blocked),
            SettingsSection::Usage => self.usage_pane(cx),
            SettingsSection::Phone => self.phone_pane(cx),
        };
        div()
            .id("settings-page")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .track_focus(&self.settings.focus)
            .bg(crate::shared::theme_colors::color(crate::shared::theme::FRAME))
            .text_color(theme.foreground)
            .text_size(px(font_size_base()))
            .child(navigation)
            .child(
                // Desktop settings panel: the workspace panel inset and radius.
                div().flex_1().min_w_0().h_full().py_1().pr_1().child(
                    div()
                        // GPUI sizes the scroll range from direct children only,
                        // so the column must keep its content height here.
                        .id(("settings-content", self.settings.section as usize))
                        .size_full()
                        .overflow_y_scroll()
                        .rounded(px(crate::app::layout::PANEL_RADIUS))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background)
                        .pt_12()
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(
                        div()
                            .w_full()
                            .max_w(px(896.))
                            .min_w_0()
                            .when(self.settings.section.owns_scroll(), |el| el.flex_1().min_h_0())
                            .when(!self.settings.section.owns_scroll(), |el| el.flex_shrink_0())
                            .when(layout.compact, |el| el.px_4().pb_8())
                            .when(!layout.compact, |el| el.px_8().pb_10())
                            .flex()
                            .flex_col()
                            .gap_8()
                            .child(
                                div()
                                    .text_size(px(if layout.compact { 24. } else { 30. }))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(self.settings.section.title()),
                            )
                            .child(content)
                            .when(read_only, |el| {
                                el.child(crate::shared::i18n::label(
                            "Application preferences are read-only in isolated Settings mode.",
                            "隔离设置模式下应用偏好设置为只读。",
                        ))
                            })
                            .when(saving, |el| {
                                el.child(crate::shared::i18n::label("Saving…", "正在保存…"))
                            })
                            .children(
                                error.map(|error| div().text_color(theme.destructive).child(error)),
                            )
                            .when(suspended && self.settings.section != SettingsSection::Subagents, |el| {
                                el.child(crate::shared::i18n::label("Preference saves are paused until the Subagents Host exits.", "子智能体 Host 退出前，偏好保存暂停。"))
                                    .child(Button::new("settings-close-manager", crate::shared::i18n::label("Close manager", "关闭管理"))
                                        .on_click(cx.listener(|this, _, _, cx| this.state.update(cx, |s, cx| s.close_profiles(cx)))))
                            })
                            .when(!matches!(self.settings.section, SettingsSection::Subagents | SettingsSection::Mcp), |el| el.child(div().flex().child(
                                Button::new(
                                    "settings-file",
                                    crate::shared::i18n::label(
                                        "Open settings file",
                                        "打开设置文件",
                                    ),
                                )
                                .disabled(blocked)
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        let path = crate::shared::settings::settings_file_path();
                                        let state = this.state.clone();
                                        crate::shared::os::file_launcher::open_in_editor(
                                            path,
                                            cx,
                                            move |error, cx| {
                                                state.update(cx, |state, cx| {
                                                    state.push_error(error.to_string());
                                                    cx.notify();
                                                });
                                            },
                                        );
                                    },
                                )),
                            ))),
                    ),
                ),
            )
            .into_any_element()
    }
}
