//! Background auto-compaction driver (SETTINGS.md "Background
//! auto-compaction"): the maintenance tick starts a compaction for idle
//! conversations whose model has it enabled, resumes the session first when
//! the release sweep or the reaper dropped it, and settles the compact ack.
//! Completion is the terminal compact marker row tagged with the command id
//! (`conversation/auto_compact.rs` `note_compact_marker`); there is no timeout.

use crate::app::store::AppState;
use crate::backend::workspace::Pending;
use crate::conversation::auto_compact::AutoCompactFlight;
use gpui::Context;
use serde_json::{Value, json};

impl AppState {
    /// One maintenance pass: drive every flight one step, then start new ones.
    pub(crate) fn run_auto_compaction(&mut self, now: std::time::Instant, cx: &mut Context<Self>) {
        let mut flying = Vec::new();
        let mut due = Vec::new();
        let mut settled = Vec::new();
        for (sid, c) in self.conversations.iter_mut() {
            if let Some(outcome) = c.auto_compact_outcome.take() {
                settled.push((sid.clone(), outcome));
            }
        }
        for (sid, outcome) in settled {
            let line = format!("auto-compact: {sid} finished ({outcome})");
            self.push_log(match outcome.as_str() {
                "failed" | "cancelled" => format!("warn: {line}"),
                _ => line,
            });
        }
        for (sid, c) in &self.conversations {
            // Subagent children are compacted by their own runtime.
            if self.child_owner.contains_key(sid) {
                continue;
            }
            if c.auto_compact.is_some() {
                flying.push(sid.clone());
                continue;
            }
            let Some(key) = crate::shared::auto_compact_settings::model_key(
                &c.config.provider,
                &c.config.model,
            ) else {
                continue;
            };
            let entry = crate::shared::auto_compact_settings::entry(&key, cx);
            if c.auto_compact_due(entry, now) {
                due.push(sid.clone());
            }
        }
        for sid in flying {
            self.advance_auto_compact(&sid, cx);
        }
        for sid in due {
            if self.session_workspace(&sid).is_none() {
                continue;
            }
            if let Some(c) = self.conversations.get_mut(&sid) {
                c.begin_auto_compact();
            }
            self.push_log(format!("auto-compact: starting for idle session {sid}"));
            self.advance_auto_compact(&sid, cx);
        }
    }

    /// Workspace owning a session: live subscription first, then the index.
    fn session_workspace(&self, sid: &str) -> Option<String> {
        let topic = format!("conversation/{sid}");
        self.workspaces
            .iter()
            .find(|ws| {
                ws.subscriptions.contains_key(&topic)
                    || ws.sessions.iter().any(|s| s.session_id == sid)
            })
            .map(|ws| ws.key.clone())
    }

    fn advance_auto_compact(&mut self, sid: &str, cx: &mut Context<Self>) {
        let Some(ws_key) = self.session_workspace(sid) else {
            return self.give_up_auto_compact(sid, "session has no workspace");
        };
        let Some(c) = self.conversations.get(sid) else {
            return;
        };
        let (flight, subscribed) = (c.auto_compact.clone(), c.subscribed);
        let Some(ws) = self.ws(&ws_key) else {
            return;
        };
        let ready = ws.started && ws.inbound.is_some();
        let pumping = ws.pumping;
        let subscribing = ws
            .pending
            .values()
            .any(|p| matches!(p, Pending::SubscribeConversation(s) if s == sid));
        let acking = ws
            .pending
            .values()
            .any(|p| matches!(p, Pending::AutoCompact(s) if s == sid));
        match flight {
            None => {}
            Some(AutoCompactFlight::Resuming {
                spawn_requested,
                subscribe_sent,
            }) => {
                if !ready {
                    if pumping {
                        return; // agent starting
                    }
                    if spawn_requested {
                        return self.give_up_auto_compact(sid, "workspace agent did not start");
                    }
                    self.set_flight(sid, true, subscribe_sent);
                    return self.ensure_spawned(&ws_key, cx);
                }
                if subscribed {
                    return self.send_auto_compact(&ws_key, sid);
                }
                if subscribing {
                    return;
                }
                if subscribe_sent {
                    return self.give_up_auto_compact(sid, "conversation subscription failed");
                }
                self.set_flight(sid, spawn_requested, true);
                // v4/conversation/subscribe cold-resumes the session in the
                // CLI gateway (resumePersistedSession); compact needs it resident.
                self.subscribe_conversation(&ws_key, sid);
            }
            Some(AutoCompactFlight::Sent { .. }) if !acking => {
                self.give_up_auto_compact(sid, "agent connection lost before the ack");
            }
            Some(AutoCompactFlight::Sent { .. }) => {}
            // The marker can only arrive over the subscription; without it
            // (agent exited, conversation dropped) the outcome is unknowable.
            Some(AutoCompactFlight::Running { .. }) if !subscribed => {
                self.give_up_auto_compact(sid, "conversation unsubscribed mid-compaction");
            }
            Some(AutoCompactFlight::Running { .. }) => {}
            // Already finished; only the idle frame is missing, and it can't
            // arrive without the subscription.
            Some(AutoCompactFlight::Settling) if !subscribed => {
                if let Some(c) = self.conversations.get_mut(sid) {
                    c.auto_compact = None;
                    c.compacted_through = c.rows.keys().next_back().copied();
                }
            }
            Some(AutoCompactFlight::Settling) => {}
        }
    }

    fn set_flight(&mut self, sid: &str, spawn_requested: bool, subscribe_sent: bool) {
        if let Some(c) = self.conversations.get_mut(sid) {
            c.auto_compact = Some(AutoCompactFlight::Resuming {
                spawn_requested,
                subscribe_sent,
            });
        }
    }

    fn send_auto_compact(&mut self, ws_key: &str, sid: &str) {
        let command_id = crate::backend::launcher::new_command_id();
        if let Some(c) = self.conversations.get_mut(sid) {
            c.auto_compact = Some(AutoCompactFlight::Sent {
                command_id: command_id.clone(),
            });
        }
        let params = crate::backend::session_cmds::command_params(
            &self.client_id,
            Some(sid),
            "compact",
            json!({}),
            None,
            None,
            command_id,
            crate::backend::launcher::now_ms(),
        );
        let sent = self.send_envelope(ws_key, params, Pending::AutoCompact(sid.to_string()));
        if !sent {
            self.give_up_auto_compact(sid, "compact command could not be written");
        } else {
            self.push_log(format!("auto-compact: sent compact for {sid}"));
        }
    }

    /// End the flight without retrying; `auto_compacted` stays set, so the
    /// next attempt waits for new activity.
    fn give_up_auto_compact(&mut self, sid: &str, reason: &str) {
        if let Some(c) = self.conversations.get_mut(sid) {
            c.auto_compact = None;
        }
        self.push_log(format!("warn: auto-compact failed for {sid}: {reason}"));
    }

    /// Settles a `Pending::AutoCompact` response. Errors go to the log, not
    /// the error banner: the user didn't ask for this compaction.
    pub(crate) fn handle_auto_compact_response(
        &mut self,
        ws_key: &str,
        id: u64,
        result: &Option<Value>,
        error: &Option<Value>,
    ) -> bool {
        let Some(ws) = self.ws_mut(ws_key) else {
            return false;
        };
        let sid = match ws.pending.get(&id) {
            Some(Pending::AutoCompact(sid)) => sid.clone(),
            _ => return false,
        };
        ws.pending.remove(&id);
        let accepted =
            error.is_none() && crate::backend::submission::ack_succeeded(result.as_ref());
        if let Some(c) = self.conversations.get_mut(&sid) {
            c.note_auto_compact_ack(accepted);
        }
        if !accepted {
            let field = |v: &Option<Value>, k: &str| {
                v.as_ref()
                    .and_then(|v| v.get(k))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            };
            let reason = field(error, "message")
                .or_else(|| field(result, "message"))
                .or_else(|| field(result, "reasonCode"))
                .or_else(|| field(result, "status"))
                .unwrap_or_else(|| "malformed ack".into());
            self.give_up_auto_compact(&sid, &reason);
        }
        true
    }
}

#[cfg(test)]
#[path = "auto_compact_tests.rs"]
mod tests;
