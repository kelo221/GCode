//! User-facing text for command ack `reasonCode`s that have a clear remedy.
//! Unlisted codes keep the raw "label status: reason" banner.
//!
//! Spec source: apps/zcode-cli/packages/bootstrap/src/zcode-protocol-v4/
//! commands/prompt-turn.ts (`MODEL_UNAVAILABLE_REASON_CODE`) and
//! commands/handlers/interaction-background.ts (`V4SubagentStopRejectedError`:
//! `fault.command.subagentStopRejected.<reason without background_task_>`).

use serde_json::Value;

pub(crate) const MODEL_UNAVAILABLE: &str = "modelUnavailable";
pub(crate) const SUBAGENT_STOP_REJECTED: &str = "fault.command.subagentStopRejected.";

/// `(English, Chinese)` explanation for a known reason code.
pub(crate) fn reason_text(reason_code: &str) -> Option<(&'static str, &'static str)> {
    if reason_code == MODEL_UNAVAILABLE {
        return Some((
            "This session's model is no longer available. Pick another model.",
            "此会话的模型已不可用，请选择其他模型。",
        ));
    }
    match reason_code.strip_prefix(SUBAGENT_STOP_REJECTED)? {
        "not_found" | "not_running" => Some((
            "Subagent not stopped: it already finished or was replaced by a newer run.",
            "子代理未停止：它已结束，或已被新的执行取代。",
        )),
        "cancel_not_supported" => Some((
            "Subagent not stopped: this subagent can't be stopped.",
            "子代理未停止：该子代理不支持停止。",
        )),
        _ => None,
    }
}

/// Localized banner for an ack whose `reasonCode` has a known explanation.
pub(crate) fn ack_reason_banner(ack: Option<&Value>) -> Option<String> {
    let code = ack?.get("reasonCode")?.as_str()?;
    let (en, zh) = reason_text(code)?;
    Some(crate::shared::i18n::label(en, zh).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn model_unavailable_has_a_remedy() {
        let (en, zh) = reason_text("modelUnavailable").unwrap();
        assert_eq!(
            en,
            "This session's model is no longer available. Pick another model."
        );
        assert!(!zh.is_empty());
    }

    #[test]
    fn subagent_stop_rejections_are_explained() {
        for reason in ["not_found", "not_running"] {
            let code = format!("{SUBAGENT_STOP_REJECTED}{reason}");
            assert!(reason_text(&code).unwrap().0.contains("already finished"));
        }
        let code = format!("{SUBAGENT_STOP_REJECTED}cancel_not_supported");
        assert!(reason_text(&code).unwrap().0.contains("can't be stopped"));
        // Unknown suffixes fall back to the raw banner.
        assert!(reason_text(&format!("{SUBAGENT_STOP_REJECTED}new_reason")).is_none());
    }

    #[test]
    fn unknown_or_missing_codes_keep_the_raw_banner() {
        assert!(reason_text("guard.runtimeInitializing").is_none());
        assert!(ack_reason_banner(None).is_none());
        assert!(ack_reason_banner(Some(&json!({"status": "rejected"}))).is_none());
        assert!(
            ack_reason_banner(Some(
                &json!({"status": "rejected", "reasonCode": "modelUnavailable"})
            ))
            .is_some()
        );
    }
}
