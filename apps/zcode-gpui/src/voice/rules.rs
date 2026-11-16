//! Pure voice decisions: whether a Speak call may play, and which pending
//! interactions earn a fallback line (VOICE.md "Product rules").

use crate::conversation::interactions::PendingInteraction;
use crate::shared::voice_settings::VoiceSettings;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RejectReason {
    Disabled,
    UnfocusedOnly,
    Unavailable,
}

impl RejectReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::UnfocusedOnly => "unfocusedOnly",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Settings gate shared by Speak calls and fallback lines.
pub(crate) fn speak_gate(
    settings: &VoiceSettings,
    window_active: bool,
) -> Result<(), RejectReason> {
    if !settings.enabled {
        return Err(RejectReason::Disabled);
    }
    if settings.only_when_unfocused && window_active {
        return Err(RejectReason::UnfocusedOnly);
    }
    Ok(())
}

/// A new turn begins when the phase becomes active from a resting phase;
/// resuming from `waiting`/`paused` (an answered prompt) continues the turn.
pub(crate) fn starts_turn(before: &str, after: &str) -> bool {
    use crate::conversation::model::phase_is_active;
    phase_is_active(after) && !phase_is_active(before) && !matches!(before, "waiting" | "paused")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FallbackKind {
    Permission,
    Question,
    PlanApproval,
}

impl FallbackKind {
    pub(crate) fn of(interaction: &PendingInteraction) -> Self {
        if interaction.is_plan_approval() {
            Self::PlanApproval
        } else if interaction.kind == "userInput" {
            Self::Question
        } else {
            // permission and workspaceHookReview both wait on an approval.
            Self::Permission
        }
    }

    pub(crate) fn line(self) -> &'static str {
        match self {
            Self::Permission => "Permission needed",
            Self::Question => "Question waiting",
            Self::PlanApproval => "Plan ready",
        }
    }
}

#[derive(Default)]
struct SessionVoice {
    spoke_this_turn: bool,
    seen: HashSet<String>,
}

/// Per-session "spoke this turn" flags and the interaction ids already seen.
#[derive(Default)]
pub(crate) struct TurnLedger {
    sessions: HashMap<String, SessionVoice>,
}

impl TurnLedger {
    pub(crate) fn turn_started(&mut self, session_id: &str) {
        self.sessions
            .entry(session_id.into())
            .or_default()
            .spoke_this_turn = false;
    }

    /// Records an accepted Speak call. `info` heads-ups don't count.
    pub(crate) fn spoke(&mut self, session_id: &str, kind: &str) {
        // 修复原因：智能体可以在回合中途用 info 先打个招呼再继续干活；若它也置位
        // spoke_this_turn，同一回合后面真正需要用户处理的权限/提问就不会再播报。
        if kind == "info" {
            return;
        }
        self.sessions
            .entry(session_id.into())
            .or_default()
            .spoke_this_turn = true;
    }

    /// Records `current` as seen and returns the fallback for the first
    /// interaction not seen before. `first_load` (a session just loaded, its
    /// prompts already waiting) and sessions that already spoke this turn
    /// record without announcing.
    pub(crate) fn fallback_for(
        &mut self,
        session_id: &str,
        current: &[PendingInteraction],
        first_load: bool,
    ) -> Option<FallbackKind> {
        let session = self.sessions.entry(session_id.into()).or_default();
        let mut fresh = None;
        for interaction in current {
            if session.seen.insert(interaction.interaction_id.clone()) && fresh.is_none() {
                fresh = Some(FallbackKind::of(interaction));
            }
        }
        if first_load || session.spoke_this_turn {
            return None;
        }
        fresh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn interaction(id: &str, kind: &str, tool: &str) -> PendingInteraction {
        PendingInteraction::from_value(&json!({
            "interactionId": id,
            "kind": kind,
            "payload": { "kind": kind, "toolName": tool, "prompt": "?", "summary": "?" }
        }))
        .expect("fixture parses")
    }

    #[test]
    fn gate_respects_enabled_and_focus() {
        let mut settings = VoiceSettings::default();
        assert_eq!(speak_gate(&settings, false), Err(RejectReason::Disabled));
        settings.enabled = true;
        assert_eq!(speak_gate(&settings, true), Ok(()));
        settings.only_when_unfocused = true;
        assert_eq!(
            speak_gate(&settings, true),
            Err(RejectReason::UnfocusedOnly)
        );
        assert_eq!(speak_gate(&settings, false), Ok(()));
    }

    #[test]
    fn turn_starts_only_from_resting_phases() {
        assert!(starts_turn("completedSuccess", "running"));
        assert!(starts_turn("", "prewarming"));
        assert!(!starts_turn("waiting", "running"));
        assert!(!starts_turn("paused", "running"));
        assert!(!starts_turn("prewarming", "running"));
        assert!(!starts_turn("running", "completedSuccess"));
    }

    #[test]
    fn classifies_fallback_kinds() {
        assert_eq!(
            FallbackKind::of(&interaction("a", "permission", "Bash")),
            FallbackKind::Permission
        );
        assert_eq!(
            FallbackKind::of(&interaction("b", "userInput", "AskUserQuestion")),
            FallbackKind::Question
        );
        assert_eq!(
            FallbackKind::of(&interaction("c", "userInput", "ExitPlanMode")),
            FallbackKind::PlanApproval
        );
    }

    #[test]
    fn announces_each_new_interaction_once() {
        let mut ledger = TurnLedger::default();
        let first = [interaction("i1", "permission", "Bash")];
        assert_eq!(
            ledger.fallback_for("s", &first, false),
            Some(FallbackKind::Permission)
        );
        assert_eq!(ledger.fallback_for("s", &first, false), None);
        let both = [
            first[0].clone(),
            interaction("i2", "userInput", "ExitPlanMode"),
        ];
        assert_eq!(
            ledger.fallback_for("s", &both, false),
            Some(FallbackKind::PlanApproval)
        );
    }

    #[test]
    fn speak_this_turn_suppresses_until_next_turn() {
        let mut ledger = TurnLedger::default();
        ledger.turn_started("s");
        ledger.spoke("s", "attention");
        let q = [interaction("q1", "userInput", "AskUserQuestion")];
        assert_eq!(ledger.fallback_for("s", &q, false), None);
        ledger.turn_started("s");
        let q2 = [interaction("q2", "userInput", "AskUserQuestion")];
        assert_eq!(
            ledger.fallback_for("s", &q2, false),
            Some(FallbackKind::Question)
        );
    }

    #[test]
    fn first_load_seeds_without_announcing() {
        let mut ledger = TurnLedger::default();
        let waiting = [interaction("old", "permission", "Bash")];
        assert_eq!(ledger.fallback_for("s", &waiting, true), None);
        assert_eq!(ledger.fallback_for("s", &waiting, false), None);
    }

    #[test]
    fn sessions_are_independent() {
        let mut ledger = TurnLedger::default();
        ledger.spoke("a", "done");
        let p = [interaction("p", "permission", "Bash")];
        assert_eq!(
            ledger.fallback_for("b", &p, false),
            Some(FallbackKind::Permission)
        );
    }

    #[test]
    fn info_heads_up_keeps_fallback() {
        let mut ledger = TurnLedger::default();
        ledger.turn_started("s");
        ledger.spoke("s", "info");
        let p = [interaction("p", "permission", "Bash")];
        assert_eq!(
            ledger.fallback_for("s", &p, false),
            Some(FallbackKind::Permission)
        );
    }
}
