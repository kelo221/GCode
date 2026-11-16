//! Settings › MCP Servers "Code mode" (apps/zcode-cli/docs/design/v2/tool/21-codemode.md):
//! the GPUI-only `gpuiCodemode` that gives sessions the `codemode` script tool.

use crate::app::settings_rows::{ControlWidth, settings_card, settings_row};
use crate::shared::i18n::label;
use crate::shared::preferences::{PreferenceChange, Preferences};
use crate::shared::settings::AppSettings;
use ely_gpui_component::forms::{Choice, Select};
use gpui::{AnyElement, IntoElement, div, prelude::*};

pub(crate) fn codemode_card(settings: &AppSettings, blocked: bool) -> AnyElement {
    let select = div().w_full().child(
        Select::new(
            "settings-codemode-mode",
            [
                Choice::new("on", label("On", "开启")),
                Choice::new("only", label("Only", "仅脚本")),
                Choice::new("off", label("Off", "关闭")),
            ],
        )
        .selected(settings.codemode_mode().to_owned())
        .disabled(blocked)
        .on_change(|value, _, cx| {
            Preferences::enqueue(PreferenceChange::CodemodeMode(value.to_string()), cx)
        }),
    );
    #[cfg(test)]
    let select =
        crate::app::test_support::track_children(select, vec!["settings-codemode-mode".into()]);
    settings_card([settings_row(
        label("Code mode", "代码模式"),
        Some(label(
            "Agents can run a short JavaScript script that calls tools and returns only what it prints, so large tool results stay out of the context window. Every call in the script still goes through permissions and hooks. Only hides the other tools and lists them inside the script tool instead. Applies to new and resumed sessions.",
            "Agent 可以运行一段调用工具的 JavaScript 脚本，只返回脚本输出的内容，大型工具结果不会进入上下文窗口。脚本内的每次调用仍经过权限检查和 Hook。“仅脚本”会隐藏其他工具，改为在脚本工具中列出。对新建和恢复的会话生效。",
        ).into()),
        select,
        ControlWidth::Default,
    )])
    .into_any_element()
}
