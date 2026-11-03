//! Provider card "Prompt cache" row (PARITY.md "Prompt cache lifetime"): the
//! GPUI-only per-provider `gpuiPromptCacheByProvider`, sent to the agent as
//! `promptCacheModes`. Only Anthropic Messages providers show it.

use crate::shared::i18n::label;
use crate::shared::preferences::{PreferenceChange, Preferences};
use crate::shared::settings::AppSettings;
use crate::shared::theme::{active_theme, font_size_xs};
use ely_gpui_component::forms::{Choice, Select};
use gpui::{AnyElement, IntoElement, div, prelude::*, px};

pub(crate) fn prompt_cache_control(
    provider: &str,
    settings: &AppSettings,
    blocked: bool,
) -> AnyElement {
    let id = provider.to_owned();
    let select = div().w(px(280.)).child(
        Select::new(
            "provider-prompt-cache",
            [
                Choice::new("5m", label("5 minutes", "5 分钟")),
                Choice::new("1h", label("1 hour", "1 小时")),
                Choice::new("off", label("Off", "关闭")),
            ],
        )
        .selected(settings.prompt_cache_mode(provider).to_owned())
        .disabled(blocked)
        .on_change(move |value, _, cx| {
            Preferences::enqueue(
                PreferenceChange::PromptCache(id.clone(), value.to_string()),
                cx,
            )
        }),
    );
    #[cfg(test)]
    let select =
        crate::app::test_support::track_children(select, vec!["provider-prompt-cache".into()]);
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(select)
        .child(
            div()
                .text_size(px(font_size_xs()))
                .text_color(active_theme().subtle)
                .child(label(
                    "1 hour costs more to write (2× input instead of 1.25×) but survives longer pauses. Off sends no cache markers, for proxies that reject them. Applies to new and resumed sessions.",
                    "1 小时写入成本更高（输入价的 2 倍而非 1.25 倍），但可跨越较长的停顿。关闭则不发送缓存标记，适用于拒绝该字段的代理。对新建和恢复的会话生效。",
                )),
        )
        .into_any_element()
}
