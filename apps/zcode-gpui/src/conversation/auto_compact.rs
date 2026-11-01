//! Idle tracking and in-flight state of GPUI's background auto-compaction
//! (SETTINGS.md "Background auto-compaction"). The maintenance tick that
//! starts and drives it lives in `app/auto_compact.rs`.

use crate::conversation::model::ConversationState;
use crate::shared::auto_compact_settings::AutoCompactEntry;
use serde_json::Value;
use std::time::{Duration, Instant};

/// Below this the next turn is cheap anyway; compacting would only lose detail.
pub(crate) const TOKEN_FLOOR: f64 = 20_000.;

/// One auto-compaction in flight for a conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AutoCompactFlight {
    /// Waiting for the agent and the conversation subscription (which
    /// cold-resumes the session in the CLI). Each step is tried once.
    Resuming {
        spawn_requested: bool,
        subscribe_sent: bool,
    },
    /// `compact` written with `command_id`; waiting for its ack.
    Sent { command_id: String },
    /// Accepted; ends when the compact marker row carrying `command_id` as
    /// `sourceCommandId` reaches a terminal status.
    Running { command_id: String },
    /// Terminal marker seen while the compaction's own turn was still
    /// active; ends when the phase next leaves active.
    // 标记行先于该轮的 control 帧收尾到达：若此时直接结束，随后同一轮的 running
    // 帧会被 note_control 当成新活动，清掉 auto_compacted 并重启空闲计时，
    // 于是每 50 分钟再压缩一次（2026-11-01 一夜 8 次空压缩）。等回到空闲再结束。
    Settling,
}

impl AutoCompactFlight {
    fn command_id(&self) -> Option<&str> {
        match self {
            Self::Sent { command_id } | Self::Running { command_id } => Some(command_id),
            Self::Resuming { .. } | Self::Settling => None,
        }
    }
}

impl ConversationState {
    /// Called on every `control` update with whether the phase is active.
    pub(crate) fn note_control(&mut self, active: bool, now: Instant) {
        if active {
            self.idle_since = None;
            match self.auto_compact {
                // A run started before the compact went out: new activity wins.
                Some(AutoCompactFlight::Resuming { .. }) => {
                    self.auto_compact = None;
                    self.auto_compacted = false;
                }
                None => self.auto_compacted = false,
                _ => {}
            }
        } else {
            self.idle_since.get_or_insert(now);
            if self.auto_compact == Some(AutoCompactFlight::Settling) {
                self.auto_compact = None;
                self.compacted_through = self.rows.keys().next_back().copied();
            }
        }
    }

    /// The compact ack: accepted moves `Sent` on; anything else ends the flight.
    pub(crate) fn note_auto_compact_ack(&mut self, accepted: bool) {
        if !accepted {
            self.auto_compact = None;
        } else if let Some(AutoCompactFlight::Sent { command_id }) = &self.auto_compact {
            self.auto_compact = Some(AutoCompactFlight::Running {
                command_id: command_id.clone(),
            });
        }
    }

    /// A row from a delta or snapshot. The CLI guarantees every accepted
    /// manual compact ends in one terminal compact marker tagged with its
    /// command id (core `settleManualCompactTimeline`), so that row is the
    /// completion signal; it may arrive before the ack.
    pub(crate) fn note_compact_marker(&mut self, row: &Value) {
        if row.get("kind").and_then(Value::as_str) != Some("timelineMarker")
            || row.pointer("/marker/type").and_then(Value::as_str) != Some("compact")
        {
            return;
        }
        let status = row
            .pointer("/marker/status")
            .and_then(Value::as_str)
            .unwrap_or("running");
        if status == "running" {
            return;
        }
        let ours = self
            .auto_compact
            .as_ref()
            .and_then(AutoCompactFlight::command_id)
            .is_some_and(|id| row.get("sourceCommandId").and_then(Value::as_str) == Some(id));
        if ours {
            self.auto_compact_outcome = Some(status.to_string());
            if self.phase_running() {
                self.auto_compact = Some(AutoCompactFlight::Settling);
            } else {
                self.auto_compact = None;
                // The marker row is inserted after this call.
                let marker_id = row.get("rowId").and_then(Value::as_u64);
                self.compacted_through = marker_id.max(self.rows.keys().next_back().copied());
            }
        }
    }

    /// Whether the maintenance tick should start an auto-compaction now.
    pub(crate) fn auto_compact_due(&self, entry: AutoCompactEntry, now: Instant) -> bool {
        let wait = Duration::from_secs(u64::from(entry.minutes) * 60);
        entry.enabled
            && !self.auto_compacted
            && self.auto_compact.is_none()
            && self
                .idle_since
                .is_some_and(|t| now.saturating_duration_since(t) >= wait)
            && self
                .context_window
                .as_ref()
                .is_some_and(|w| w.used >= TOKEN_FLOOR)
            && !crate::app::conversation_release::has_live_work(self)
            && self.has_rows_since_compaction()
    }

    /// Nothing appended since the last auto-compaction means there is
    /// nothing new to compact; the CLI would only record an empty turn.
    fn has_rows_since_compaction(&self) -> bool {
        self.compacted_through.is_none_or(|through| {
            self.rows
                .keys()
                .next_back()
                .is_some_and(|&last| last > through)
        })
    }

    /// Mark the compaction started; it won't fire again until a new run.
    pub(crate) fn begin_auto_compact(&mut self) {
        self.auto_compacted = true;
        self.auto_compact = Some(AutoCompactFlight::Resuming {
            spawn_requested: false,
            subscribe_sent: false,
        });
    }
}

#[cfg(test)]
#[path = "auto_compact_tests.rs"]
mod tests;
