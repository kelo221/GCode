use crate::app::{root::RootView, settings::SettingsSection};
use crate::shared::i18n::label;
use crate::shared::theme::{IconName, active_theme, font_size_sm, ui_size};
use ely_gpui_component::forms::{Input, InputEvent, TextInput};
use gpui::{AnyElement, Context, IntoElement, Window, div, prelude::*, px};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsGroup {
    Basics,
    AgentCapabilities,
    DataAndStats,
}

impl SettingsGroup {
    /// Desktop `settings.sidebar.group.*`.
    fn title(self) -> &'static str {
        match self {
            Self::Basics => label("Basics", "基础设置"),
            Self::AgentCapabilities => label("Agent capabilities", "Agent 能力"),
            Self::DataAndStats => label("Data and statistics", "数据与统计"),
        }
    }
}

impl SettingsSection {
    pub(crate) fn title(self) -> &'static str {
        let (en, zh) = self.labels();
        label(en, zh)
    }

    /// Desktop section titles (`settingsPageConfig.ts` title ids).
    fn labels(self) -> (&'static str, &'static str) {
        match self {
            Self::General => ("General", "常规"),
            Self::Appearance => ("Appearance", "外观"),
            Self::Shortcuts => ("Keyboard Shortcuts", "键盘快捷键"),
            Self::Memory => ("Memory", "记忆"),
            Self::Models => ("Model settings", "模型设置"),
            Self::Subagents => ("Subagents", "子智能体"),
            Self::Plugins => ("Plugins", "插件"),
            Self::Mcp => ("MCP Servers", "MCP 服务器"),
            Self::Browser => ("Browser Use", "浏览器使用"),
            Self::Voice => ("Voice", "语音"),
            Self::SystemPrompt => ("System prompt", "系统提示词"),
            Self::Secrets => ("Secrets", "密钥"),
            Self::CodeSearch => ("Code search", "代码搜索"),
            Self::Diagnostics => ("Diagnostics", "诊断"),
            Self::Usage => ("Usage stats", "使用统计"),
            Self::Phone => ("Phone", "手机"),
        }
    }

    /// Desktop `settingsPageConfig.ts` section icons.
    pub(crate) fn icon_path(self) -> &'static str {
        use crate::shared::app_assets::{BLOCKS_ICON, BRAIN_ICON, CABLE_ICON, SETTINGS_2_ICON};
        match self {
            Self::General => SETTINGS_2_ICON,
            Self::Appearance => IconName::Palette.path(),
            Self::Shortcuts => IconName::Keyboard.path(),
            Self::Memory => BRAIN_ICON,
            Self::Models => IconName::Package.path(),
            Self::Subagents => IconName::Bot.path(),
            Self::Plugins => BLOCKS_ICON,
            Self::Mcp => CABLE_ICON,
            Self::Browser => IconName::Globe.path(),
            Self::Voice => IconName::Volume2.path(),
            Self::SystemPrompt => IconName::FileText.path(),
            Self::Secrets => IconName::Key.path(),
            Self::CodeSearch => IconName::Search.path(),
            Self::Diagnostics => IconName::TriangleAlert.path(),
            Self::Usage => IconName::ChartColumn.path(),
            Self::Phone => IconName::Smartphone.path(),
        }
    }

    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::General => "settings-general",
            Self::Appearance => "settings-appearance",
            Self::Shortcuts => "settings-shortcuts",
            Self::Memory => "settings-memory",
            Self::Models => "settings-models",
            Self::Subagents => "settings-subagents",
            Self::Plugins => "settings-plugins",
            Self::Mcp => "settings-mcp",
            Self::Browser => "settings-browser",
            Self::Voice => "settings-voice",
            Self::SystemPrompt => "settings-system-prompt",
            Self::Secrets => "settings-secrets",
            Self::CodeSearch => "settings-code-search",
            Self::Diagnostics => "settings-diagnostics",
            Self::Usage => "settings-usage",
            Self::Phone => "settings-phone",
        }
    }
}

pub(crate) fn search_matches(query: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    query
        .split_whitespace()
        .all(|word| text.contains(&word.to_lowercase()))
}

pub(crate) fn navigation_sections(query: &str) -> Vec<(SettingsGroup, SettingsSection)> {
    use SettingsGroup::*;
    use SettingsSection::*;
    [
        (Basics, General),
        (Basics, Appearance),
        (Basics, Models),
        (Basics, Shortcuts),
        (AgentCapabilities, Memory),
        (AgentCapabilities, Subagents),
        (AgentCapabilities, Plugins),
        (AgentCapabilities, Mcp),
        (AgentCapabilities, Browser),
        (AgentCapabilities, Voice),
        (AgentCapabilities, SystemPrompt),
        (AgentCapabilities, Secrets),
        (AgentCapabilities, CodeSearch),
        (AgentCapabilities, Diagnostics),
        (DataAndStats, Usage),
        (DataAndStats, Phone),
    ]
    .into_iter()
    .filter(|(_, section)| {
        let (en, zh) = section.labels();
        search_matches(query, &format!("{en} {zh}"))
    })
    .collect()
}

/// Desktop collapses the 268px navigation to a 68px icon rail below `lg`.
pub(crate) struct SettingsLayout {
    pub compact: bool,
    pub navigation_width: f32,
}

pub(crate) fn settings_layout(width: f32) -> SettingsLayout {
    let compact = width < 1024.;
    SettingsLayout {
        compact,
        navigation_width: if compact { 68. } else { 268. },
    }
}

/// One navigation row (Desktop `SettingsSidebarButton`): 32px, `rounded-xl`,
/// 16px icon and the label, which the compact rail hides.
pub(crate) fn nav_item(
    id: &'static str,
    icon_path: &'static str,
    text: &'static str,
    active: bool,
    compact: bool,
) -> gpui::Stateful<gpui::Div> {
    let theme = active_theme();
    div()
        .id(id)
        .h(px(32.))
        .flex()
        .items_center()
        .gap_2()
        .rounded_xl()
        .when(compact, |el| el.w(px(40.)).h(px(40.)).justify_center())
        .when(!compact, |el| el.w_full().px(px(10.)))
        .cursor_pointer()
        .text_size(px(ui_size(14.)))
        .text_color(theme.foreground)
        .when(active, |el| el.bg(theme.surface_hover))
        .when(!active, |el| el.hover(|s| s.bg(theme.surface_hover)))
        .child(
            gpui::svg()
                .path(icon_path)
                .size(px(16.))
                .flex_none()
                .text_color(theme.foreground),
        )
        .when(compact, |el| {
            el.tooltip(ely_gpui_component::primitives::Tooltip::text(text))
        })
        .when(!compact, |el| {
            el.child(div().min_w_0().truncate().child(text))
        })
}

#[derive(Clone, Copy)]
pub(crate) enum SettingsSearch {
    Navigation,
    Shortcuts,
    Models,
}

impl SettingsSearch {
    fn id(self) -> &'static str {
        match self {
            Self::Navigation => "settings-search",
            Self::Shortcuts => "settings-shortcut-search",
            Self::Models => "settings-model-search",
        }
    }
    fn placeholder(self) -> &'static str {
        match self {
            Self::Navigation => label("Search settings", "搜索设置"),
            Self::Shortcuts => label("Search shortcuts", "搜索快捷键"),
            Self::Models => label(
                "Search provider, model or reasoning",
                "搜索提供商、模型或推理级别",
            ),
        }
    }
    fn query(self, view: &RootView) -> &str {
        match self {
            Self::Navigation => &view.settings.query,
            Self::Shortcuts => &view.settings.shortcut_query,
            Self::Models => &view.settings.model_query,
        }
    }
    fn set(self, view: &mut RootView, text: String) {
        match self {
            Self::Navigation => view.settings.query = text,
            Self::Shortcuts => view.settings.shortcut_query = text,
            Self::Models => view.settings.model_query = text,
        }
    }
}

impl RootView {
    pub(crate) fn settings_search(
        &mut self,
        field: SettingsSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = field.id();
        let text = field.query(self).to_owned();
        // Ely entity 仅投影 RootView 的搜索草稿；重新进入 section 时从同一 owner 恢复。
        let input = window.use_keyed_state(id, cx, |window, cx| {
            let mut input = TextInput::new(window, cx).placeholder(field.placeholder());
            input.set_text(text, cx);
            input
        });
        // keyed 输入会跨语言切换保留；只更新提示投影，不能重建 entity 丢失搜索草稿或焦点。
        if input.read(cx).placeholder_text().as_ref() != field.placeholder() {
            input.update(cx, |input, cx| {
                input.set_placeholder(field.placeholder(), cx)
            });
        }
        #[cfg(test)]
        if cx.has_global::<crate::app::test_support::TestLabels>() {
            let placeholder = input.read(cx).placeholder_text().to_string();
            cx.global_mut::<crate::app::test_support::TestLabels>()
                .0
                .insert(id.into(), placeholder);
        }
        let view = cx.entity().downgrade();
        window.use_keyed_state(format!("{id}-subscription"), cx, |_, cx| {
            cx.subscribe(&input, move |_, input, event: &InputEvent, cx| {
                if *event == InputEvent::Changed {
                    let text = input.read(cx).text().to_owned();
                    let _ = view.update(cx, |view, cx| {
                        field.set(view, text);
                        cx.notify();
                    });
                }
            })
        });
        let wrapper = div().w_full().min_w_0().child(Input::new(&input));
        #[cfg(test)]
        let wrapper = crate::app::test_support::track_children(wrapper, vec![id.into()]);
        wrapper.into_any_element()
    }

    pub(crate) fn render_settings_navigation(
        &mut self,
        compact: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let search =
            (!compact).then(|| self.settings_search(SettingsSearch::Navigation, window, cx));
        let mut groups = div().flex().flex_col().gap_4();
        let mut sections = navigation_sections(&self.settings.query);
        // The phone drives the desktop's browser and has no voice output of
        // its own; both are desktop-only sections.
        if self.state.read(cx).phone_client.enabled {
            sections.retain(|(_, s)| {
                !matches!(
                    s,
                    SettingsSection::Browser
                        | SettingsSection::Voice
                        | SettingsSection::SystemPrompt
                        | SettingsSection::Secrets
                        | SettingsSection::CodeSearch
                        | SettingsSection::Diagnostics
                )
            });
        }
        for (index, group) in [
            SettingsGroup::Basics,
            SettingsGroup::AgentCapabilities,
            SettingsGroup::DataAndStats,
        ]
        .into_iter()
        .enumerate()
        {
            let entries: Vec<_> = sections
                .iter()
                .filter(|(g, _)| *g == group)
                .map(|(_, s)| *s)
                .collect();
            if entries.is_empty() {
                continue;
            }
            let mut buttons = div().flex().flex_col().gap_1();
            for section in &entries {
                let section = *section;
                buttons =
                    buttons.child(
                        nav_item(
                            section.id(),
                            section.icon_path(),
                            section.title(),
                            self.settings.section == section,
                            compact,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_settings_section(section, cx)
                        })),
                    );
            }
            #[cfg(test)]
            let buttons = crate::app::test_support::track_children(
                buttons,
                entries.iter().map(|s| s.id().to_owned()).collect(),
            );
            groups = groups.child(
                div()
                    .flex()
                    .flex_col()
                    .when(compact, |el| el.items_center())
                    // The rail separates groups with a rule instead of labels.
                    .when(compact && index > 0, |el| {
                        el.pt_3().border_t_1().border_color(active_theme().border)
                    })
                    .when(!compact, |el| {
                        el.child(
                            div()
                                .px(px(10.))
                                .pb_1()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(active_theme().subtlest)
                                .text_size(px(font_size_sm()))
                                .child(group.title()),
                        )
                    })
                    .child(buttons),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_4()
            .children(search)
            .child(groups)
            .when(sections.is_empty(), |el| {
                el.child(label("No matching settings", "没有匹配的设置"))
            })
            .into_any_element()
    }
}
