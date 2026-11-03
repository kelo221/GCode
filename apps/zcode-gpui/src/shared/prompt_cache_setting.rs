//! `gpui_settings.json` `gpuiPromptCacheByProvider`: the Anthropic prompt cache
//! lifetime per provider ID (PARITY.md "Prompt cache lifetime"), sent to the
//! agent as `promptCacheModes`.

use crate::shared::settings::AppSettings;
use serde_json::{Map, Value};

/// Values the CLI accepts (`zcodeSessionRuntimePreferencesResultSchema.promptCacheModes`).
pub(crate) const PROMPT_CACHE_MODES: [&str; 3] = ["5m", "1h", "off"];
const DEFAULT_MODE: &str = "5m";

/// Missing or unknown values mean `5m`, the markers the CLI already places.
pub(crate) fn parse_prompt_cache_mode(value: Option<&str>) -> &'static str {
    value
        .and_then(|v| PROMPT_CACHE_MODES.iter().find(|m| **m == v))
        .copied()
        .unwrap_or(DEFAULT_MODE)
}

impl AppSettings {
    pub(crate) fn prompt_cache_mode(&self, provider: &str) -> &'static str {
        let saved = self.gpui_prompt_cache_by_provider.as_ref();
        parse_prompt_cache_mode(saved.and_then(|m| m.get(provider)).map(String::as_str))
    }

    /// The `promptCacheModes` answer: only valid, non-default entries.
    pub(crate) fn prompt_cache_modes_json(&self) -> Value {
        let mut modes = Map::new();
        for (provider, mode) in self.gpui_prompt_cache_by_provider.iter().flatten() {
            let mode = parse_prompt_cache_mode(Some(mode));
            if !provider.is_empty() && mode != DEFAULT_MODE {
                modes.insert(provider.clone(), Value::from(mode));
            }
        }
        Value::Object(modes)
    }

    /// 5 minutes is the default, so it removes the entry instead of storing it.
    pub(crate) fn set_prompt_cache_mode(&mut self, provider: &str, mode: &str) {
        let modes = self.gpui_prompt_cache_by_provider.get_or_insert_default();
        if mode == DEFAULT_MODE {
            modes.remove(provider);
        } else {
            modes.insert(provider.to_owned(), mode.to_owned());
        }
        if modes.is_empty() {
            self.gpui_prompt_cache_by_provider = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_provider_keeps_its_own_mode_and_defaults_to_five_minutes() {
        let mut settings = AppSettings::default();
        settings.set_prompt_cache_mode("anthropic", "1h");
        settings.set_prompt_cache_mode("proxy", "off");
        assert_eq!(settings.prompt_cache_mode("anthropic"), "1h");
        assert_eq!(settings.prompt_cache_mode("proxy"), "off");
        assert_eq!(settings.prompt_cache_mode("other"), "5m");
        assert_eq!(
            settings.prompt_cache_modes_json(),
            serde_json::json!({ "anthropic": "1h", "proxy": "off" })
        );

        settings.set_prompt_cache_mode("anthropic", "5m");
        settings.set_prompt_cache_mode("proxy", "5m");
        assert_eq!(settings.gpui_prompt_cache_by_provider, None);
        assert_eq!(settings.prompt_cache_modes_json(), serde_json::json!({}));
    }

    #[test]
    fn unknown_saved_modes_are_not_sent() {
        let settings = AppSettings {
            gpui_prompt_cache_by_provider: Some(
                [("p".to_owned(), "24h".to_owned())].into_iter().collect(),
            ),
            ..Default::default()
        };
        assert_eq!(settings.prompt_cache_mode("p"), "5m");
        assert_eq!(settings.prompt_cache_modes_json(), serde_json::json!({}));
    }

    #[test]
    fn saved_in_the_gpui_file_not_the_shared_one() {
        let dir = std::env::temp_dir().join(format!(
            "zcode_gpui_prompt_cache_{}_{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let file = dir.join("setting.json");
        crate::shared::settings::update_settings_at(
            &file,
            || true,
            |s| s.set_prompt_cache_mode("anthropic", "1h"),
        )
        .expect("GPUI-only fields save while the desktop runs");
        assert!(!file.exists(), "setting.json must stay untouched");
        let own =
            std::fs::read_to_string(crate::shared::settings::gpui_settings_path(&file)).unwrap();
        assert!(own.contains(r#""gpuiPromptCacheByProvider""#), "{own}");
        let loaded = crate::shared::settings::load_settings_from_path(&file);
        assert_eq!(loaded.prompt_cache_mode("anthropic"), "1h");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_old_global_string_key_is_ignored() {
        let dir = std::env::temp_dir().join(format!(
            "zcode_gpui_prompt_cache_old_{}_{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("setting.json");
        std::fs::write(
            crate::shared::settings::gpui_settings_path(&file),
            r#"{"gpuiPromptCache":"1h","gpuiNoiseFree":false}"#,
        )
        .unwrap();
        // 旧版全局字符串值不能让整个 GPUI 偏好文件读失败。
        let strict = crate::shared::settings::read_settings_strict(&file).unwrap();
        assert_eq!(strict.gpui_noise_free, Some(false));
        assert_eq!(strict.prompt_cache_mode("anything"), "5m");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
