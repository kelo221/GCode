//! GPUI-only preferences persisted in `gpui_settings.json` beside `setting.json`
//! (split from settings.rs for the 400-line cap).

use crate::shared::settings::{AppSettings, LastComposerModel};
use serde::{Deserialize, Serialize};

/// The GPUI-only fields of [`AppSettings`], persisted in
/// [`gpui_settings_path`](crate::shared::settings::gpui_settings_path) instead of `setting.json`.
///
/// 修复原因：Electron 桌面端启动时按自己的类型整份重写 `setting.json`，丢弃不认识的键
/// （2026-10-21 日志：Electron 18:43:01 写入的内容不含 `gpuiVoice`，13 秒后启动的 GPUI
/// 读到默认值，语音因此在重启后变成关闭）。GPUI 专属偏好改存独立文件，Electron 不会碰；
/// 旧版写在 `setting.json` 的值在新文件缺少该字段时仍作为回退读取，并在下次写入时迁移。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub(super) struct GpuiOnlySettings {
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "gpuiVoice")]
    voice: Option<crate::shared::voice_settings::VoiceSettings>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "gpuiNoiseFree"
    )]
    noise_free: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "gpuiCodeSearch"
    )]
    code_search: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "gpuiLspDiagnostics"
    )]
    lsp_diagnostics: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "gpuiCodemode"
    )]
    codemode: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "gpuiLastComposerModel"
    )]
    last_composer_model: Option<LastComposerModel>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "gpuiAutoCompact"
    )]
    auto_compact: Option<crate::shared::auto_compact_settings::AutoCompactMap>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "gpuiPromptCacheByProvider"
    )]
    prompt_cache: Option<std::collections::BTreeMap<String, String>>,
}

impl GpuiOnlySettings {
    /// Moves the GPUI-only fields out of `settings`, leaving them unset.
    pub(super) fn take(settings: &mut AppSettings) -> Self {
        Self {
            voice: settings.gpui_voice.take(),
            noise_free: settings.gpui_noise_free.take(),
            code_search: settings.gpui_code_search.take(),
            lsp_diagnostics: settings.gpui_lsp_diagnostics.take(),
            codemode: settings.gpui_codemode.take(),
            last_composer_model: settings.gpui_last_composer_model.take(),
            auto_compact: settings.gpui_auto_compact.take(),
            prompt_cache: settings.gpui_prompt_cache_by_provider.take(),
        }
    }

    /// Fields set here win over legacy values read from `setting.json`.
    pub(super) fn overlay(self, settings: &mut AppSettings) {
        if self.voice.is_some() {
            settings.gpui_voice = self.voice;
        }
        if self.noise_free.is_some() {
            settings.gpui_noise_free = self.noise_free;
        }
        if self.code_search.is_some() {
            settings.gpui_code_search = self.code_search;
        }
        if self.lsp_diagnostics.is_some() {
            settings.gpui_lsp_diagnostics = self.lsp_diagnostics;
        }
        if self.codemode.is_some() {
            settings.gpui_codemode = self.codemode;
        }
        if self.last_composer_model.is_some() {
            settings.gpui_last_composer_model = self.last_composer_model;
        }
        if self.auto_compact.is_some() {
            settings.gpui_auto_compact = self.auto_compact;
        }
        if self.prompt_cache.is_some() {
            settings.gpui_prompt_cache_by_provider = self.prompt_cache;
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}
