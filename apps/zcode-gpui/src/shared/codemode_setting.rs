//! GPUI-only `gpuiCodemode` (apps/zcode-cli/docs/design/v2/tool/21-codemode.md):
//! whether sessions get the built-in `codemode` script tool.

use crate::shared::settings::AppSettings;

/// Values the agent accepts (`codemodeMode`); anything else means the default `on`.
pub(crate) const CODEMODE_MODES: [&str; 3] = ["off", "on", "only"];

pub(crate) fn parse_codemode_mode(value: Option<&str>) -> &'static str {
    value
        .and_then(|v| CODEMODE_MODES.iter().find(|m| **m == v))
        .copied()
        .unwrap_or("on")
}

impl AppSettings {
    pub(crate) fn codemode_mode(&self) -> &'static str {
        parse_codemode_mode(self.gpui_codemode.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_or_missing_modes_mean_on() {
        assert_eq!(parse_codemode_mode(Some("off")), "off");
        assert_eq!(parse_codemode_mode(Some("only")), "only");
        assert_eq!(parse_codemode_mode(Some("auto")), "on");
        assert_eq!(parse_codemode_mode(None), "on");
    }

    #[test]
    fn the_gpui_key_round_trips() {
        let settings: AppSettings = serde_json::from_str(r#"{"gpuiCodemode":"only"}"#).unwrap();
        assert_eq!(settings.codemode_mode(), "only");
        assert_eq!(AppSettings::default().codemode_mode(), "on");
    }
}
