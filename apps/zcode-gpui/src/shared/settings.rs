//! Settings reader, writer, and desktop running guard for zcode-gpui.
//! Persists to ~/.zcode/v2/setting.json.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub(crate) use crate::shared::last_composer_model::LastComposerModel;

use crate::shared::gpui_only_settings::GpuiOnlySettings;
pub use crate::shared::settings_error::SettingsError;

/// Typed view of the desktop's `setting.json`. Unset fields are omitted on
/// write (never `null`), and unmodeled keys round-trip through `extra`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct AppSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "localePreference"
    )]
    pub locale_preference: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "themePreference"
    )]
    pub theme_preference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "uiFontSize"
    )]
    pub ui_font_size: Option<f32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "shortcutBindings"
    )]
    pub shortcut_bindings: Option<HashMap<String, Vec<String>>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "keepAwakeWhileRunning"
    )]
    pub keep_awake_while_running: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "memoryEnabled"
    )]
    pub memory_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "toolSearchMode")]
    pub tool_search_mode: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "closeToTrayOnWindows"
    )]
    pub close_to_tray_on_windows: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "recentProjects"
    )]
    pub recent_projects: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "dataBaseDir"
    )]
    pub data_base_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "gpuiVoice")]
    pub gpui_voice: Option<crate::shared::voice_settings::VoiceSettings>,
    /// GPUI-only noise-free transcript; absent means on.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "gpuiNoiseFree"
    )]
    pub gpui_noise_free: Option<bool>,
    /// GPUI-only built-in fff code search (FindFiles/SearchCode); absent means on.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "gpuiCodeSearch"
    )]
    pub gpui_code_search: Option<bool>,
    /// GPUI-only language-server diagnostics after edits; absent means on.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "gpuiLspDiagnostics"
    )]
    pub gpui_lsp_diagnostics: Option<bool>,
    /// GPUI-only built-in codemode tool (`off` | `on` | `only`); absent means `on`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "gpuiCodemode"
    )]
    pub gpui_codemode: Option<String>,
    /// GPUI-only model and thinking level of the last accepted message; seeds new drafts.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "gpuiLastComposerModel"
    )]
    pub gpui_last_composer_model: Option<LastComposerModel>,
    /// GPUI-only per-model background auto-compaction; absent entries mean off.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "gpuiAutoCompact"
    )]
    pub gpui_auto_compact: Option<crate::shared::auto_compact_settings::AutoCompactMap>,
    /// GPUI-only Anthropic prompt cache lifetime per provider ID (`1h` | `off`); absent means `5m`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "gpuiPromptCacheByProvider"
    )]
    pub gpui_prompt_cache_by_provider: Option<std::collections::BTreeMap<String, String>>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl AppSettings {
    pub fn noise_free(&self) -> bool {
        self.gpui_noise_free.unwrap_or(true)
    }

    pub fn code_search(&self) -> bool {
        self.gpui_code_search.unwrap_or(true)
    }

    pub fn lsp_diagnostics(&self) -> bool {
        self.gpui_lsp_diagnostics.unwrap_or(true)
    }

    pub(crate) fn language_preference(&self) -> crate::shared::i18n::LocalePreference {
        // selector 与运行时必须使用同一回退规则；无效偏好不能偷偷跟随系统或旧 locale。
        self.locale_preference
            .as_deref()
            .or(self.locale.as_deref())
            .map(crate::shared::i18n::LocalePreference::parse)
            .unwrap_or_default()
    }
}

/// Resolve the user directory holding `.zcode/v2/setting.json`.
pub fn resolve_user_home_dir() -> PathBuf {
    if let Some(isolated) = crate::shared::isolation::active() {
        return isolated.home();
    }
    crate::shared::data_paths::DataPaths::resolve(|k| std::env::var(k).ok(), None).settings_home
}

pub fn settings_file_path() -> PathBuf {
    resolve_user_home_dir()
        .join(".zcode")
        .join("v2")
        .join("setting.json")
}

/// The GPUI-only preferences file next to `setting.json`.
pub fn gpui_settings_path(settings_path: &std::path::Path) -> PathBuf {
    settings_path.with_file_name("gpui_settings.json")
}

pub use crate::shared::desktop_lock::is_desktop_running;

/// Read settings from disk, returning defaults on any error or missing file.
pub fn load_settings() -> AppSettings {
    load_settings_from_path(&settings_file_path())
}

pub fn load_settings_from_path(path: &std::path::Path) -> AppSettings {
    let mut settings = std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str::<AppSettings>(&content).ok())
        .unwrap_or_default();
    if let Some(own) = std::fs::read_to_string(gpui_settings_path(path))
        .ok()
        .and_then(|content| serde_json::from_str::<GpuiOnlySettings>(&content).ok())
    {
        own.overlay(&mut settings);
    }
    settings
}

/// Both files, failing instead of falling back to defaults: callers write the
/// result back, and a guessed default would overwrite the user's values.
pub(crate) fn read_settings_strict(path: &std::path::Path) -> Result<AppSettings, SettingsError> {
    let mut settings = match std::fs::read_to_string(path) {
        Ok(text) => {
            serde_json::from_str::<AppSettings>(&text).map_err(|_| SettingsError::Unreadable)?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => AppSettings::default(),
        Err(e) => return Err(e.into()),
    };
    match std::fs::read_to_string(gpui_settings_path(path)) {
        Ok(text) => serde_json::from_str::<GpuiOnlySettings>(&text)
            .map_err(|_| SettingsError::Unreadable)?
            .overlay(&mut settings),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(settings)
}

/// Write via a temp file and rename; `abort` is checked right before the
/// rename and, when true, discards the write.
fn write_atomic(
    path: &std::path::Path,
    text: &str,
    abort: impl Fn() -> bool,
) -> Result<(), SettingsError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension(format!("tmp.{}", uuid::Uuid::now_v7()));
    if let Err(error) = std::fs::write(&tmp_path, text) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(error.into());
    }
    if abort() {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(SettingsError::DesktopRunning);
    }
    if let Err(error) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(error.into());
    }
    Ok(())
}

/// Apply `change` to the settings on disk. Changes to `setting.json` happen
/// only while no desktop instance runs: the desktop rewrites that file whole
/// from memory (`settingService.ts`), so a write while it runs is silently
/// lost. GPUI-only fields live in [`gpui_settings_path`] and save any time.
pub fn update_settings(change: impl FnOnce(&mut AppSettings)) -> Result<(), SettingsError> {
    update_settings_at(&settings_file_path(), is_desktop_running, change)
}

/// Read-modify-write from the files' current contents (never from a stale
/// in-memory copy), re-checking the desktop lock right before the rename.
pub fn update_settings_at(
    path: &std::path::Path,
    desktop_active: impl Fn() -> bool,
    change: impl FnOnce(&mut AppSettings),
) -> Result<(), SettingsError> {
    update_settings_checked_at(path, desktop_active, |s| {
        change(s);
        Ok(())
    })
}

pub(crate) fn update_settings_checked_at(
    path: &std::path::Path,
    desktop_active: impl Fn() -> bool,
    change: impl FnOnce(&mut AppSettings) -> Result<(), SettingsError>,
) -> Result<(), SettingsError> {
    let mut settings = read_settings_strict(path)?;
    let mut shared_before = settings.clone();
    let own_before = GpuiOnlySettings::take(&mut shared_before);
    change(&mut settings)?;
    let mut shared_after = settings;
    let own_after = GpuiOnlySettings::take(&mut shared_after);

    let shared_changed = shared_after != shared_before;
    if shared_changed && desktop_active() {
        return Err(SettingsError::DesktopRunning);
    }
    let own_path = gpui_settings_path(path);
    // Also written when only legacy `setting.json` values exist, so they move
    // over before the shared write below drops them.
    if own_after != own_before || (!own_after.is_empty() && !own_path.exists()) {
        write_atomic(
            &own_path,
            &serde_json::to_string_pretty(&own_after)?,
            || false,
        )?;
    }
    if shared_changed {
        write_atomic(
            path,
            &serde_json::to_string_pretty(&shared_after)?,
            desktop_active,
        )?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
