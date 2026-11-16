//! `AppState` glue for voice: answers `interaction/speak`, owns the sidecar
//! lifecycle (lazy spawn, events, idle unload) and announces fallback lines
//! after conversation frames apply.

use super::process::{self, Sidecar, SidecarEvent};
use super::rules::{self, RejectReason};
use super::{VoiceStatus, settings};
use crate::app::store::AppState;
use crate::shared::voice_settings::VoiceSettings;
use futures::StreamExt;
use gpui::Context;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const IDLE_CHECK: Duration = Duration::from_secs(5);

/// Conversation state captured before a frame applies.
pub(crate) struct VoiceBefore {
    session_id: String,
    first_load: bool,
    phase: String,
}

impl AppState {
    /// Answers `interaction/speak` on the desktop. Returns false without a
    /// voice service so `reverse_rpc` replies `unavailable`.
    pub(crate) fn route_voice_rpc(
        &mut self,
        ws_key: &str,
        id: &Value,
        method: &str,
        params: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if method != "interaction/speak" {
            return false;
        }
        let Some(window_active) = self.voice.as_ref().map(|v| v.window_active) else {
            return false;
        };
        let session_id = params
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("");
        let message = params
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let kind = params.get("kind").and_then(Value::as_str).unwrap_or("");
        let settings = settings(cx);
        let outcome = rules::speak_gate(&settings, window_active).and_then(|()| {
            if message.is_empty() {
                return Err(RejectReason::Unavailable);
            }
            self.voice_say(message, &settings, cx)
        });
        let result = match outcome {
            Ok(()) => {
                if let Some(voice) = self.voice.as_mut() {
                    voice.ledger.spoke(session_id, kind);
                }
                json!({ "accepted": true })
            }
            Err(reason) => json!({ "accepted": false, "reason": reason.as_str() }),
        };
        if let Some(ws) = self.ws_mut(ws_key) {
            ws.send_line(json!({ "id": id, "result": result }).to_string());
        }
        cx.notify();
        true
    }

    /// Speaks `text` with the given settings (Speak tool, fallback lines and
    /// the Settings test button). A failed engine is respawned to retry, but
    /// the call itself reports `unavailable` rather than claim it was spoken.
    pub(crate) fn voice_say(
        &mut self,
        text: &str,
        settings: &VoiceSettings,
        cx: &mut Context<Self>,
    ) -> Result<(), RejectReason> {
        let was_failed = self
            .voice
            .as_ref()
            .is_some_and(|v| matches!(v.status, VoiceStatus::Failed { .. }));
        self.ensure_voice_sidecar(cx)?;
        let voice = self.voice.as_mut().ok_or(RejectReason::Unavailable)?;
        voice.next_id += 1;
        voice.last_activity = Instant::now();
        let command = json!({
            "op": "speak",
            "id": format!("v{}", voice.next_id),
            "text": text,
            "voice": settings.voice,
            "speed": settings.speed,
            "volume": settings.volume,
        });
        let sent = voice.sidecar.as_mut().map(|s| s.send(&command));
        if !matches!(sent, Some(Ok(()))) {
            voice.shutdown();
            return Err(RejectReason::Unavailable);
        }
        voice.pending += 1;
        if was_failed {
            return Err(RejectReason::Unavailable);
        }
        Ok(())
    }

    /// Stops current speech and drops queued lines (voice turned off).
    pub(crate) fn voice_stop(&mut self) {
        if let Some(sidecar) = self.voice.as_mut().and_then(|v| v.sidecar.as_mut()) {
            let _ = sidecar.send(&json!({ "op": "stop" }));
        }
    }

    pub(crate) fn set_voice_window_active(&mut self, active: bool) {
        if let Some(voice) = self.voice.as_mut() {
            voice.window_active = active;
        }
    }

    fn ensure_voice_sidecar(&mut self, cx: &mut Context<Self>) -> Result<(), RejectReason> {
        let voice = self.voice.as_mut().ok_or(RejectReason::Unavailable)?;
        if voice.sidecar.is_some() {
            return Ok(());
        }
        let Some(exe) = process::locate() else {
            voice.status = VoiceStatus::Missing;
            return Err(RejectReason::Unavailable);
        };
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let sidecar = match Sidecar::spawn(&exe, &process::model_dir(), tx) {
            Ok(sidecar) => sidecar,
            Err(error) => {
                voice.status = VoiceStatus::Failed {
                    code: "spawn_failed".into(),
                    message: error.to_string(),
                };
                return Err(RejectReason::Unavailable);
            }
        };
        voice.generation += 1;
        let generation = voice.generation;
        voice.sidecar = Some(sidecar);
        voice.status = VoiceStatus::Loading;
        voice.last_activity = Instant::now();
        self.push_log(format!("voice sidecar started: {}", exe.display()));
        cx.spawn(async move |this, cx| {
            while let Some(event) = rx.next().await {
                let alive = this
                    .update(cx, |state, cx| {
                        state.handle_voice_event(generation, event, cx)
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(IDLE_CHECK).await;
                let keep = this
                    .update(cx, |state, cx| state.check_voice_idle(generation, cx))
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        })
        .detach();
        Ok(())
    }

    fn handle_voice_event(&mut self, generation: u64, event: SidecarEvent, cx: &mut Context<Self>) {
        let Some(voice) = self.voice.as_mut().filter(|v| v.generation == generation) else {
            return;
        };
        let mut log = None;
        match event {
            SidecarEvent::Event(event) => {
                voice.apply_event(&event);
                if event.get("event").and_then(Value::as_str) == Some("error") {
                    log = Some(format!("voice: {event}"));
                }
                // A failed engine can't recover in place; the next speak
                // respawns it to retry.
                if matches!(voice.status, VoiceStatus::Failed { .. }) {
                    voice.sidecar = None;
                }
            }
            SidecarEvent::Exited => voice.shutdown(),
        }
        if let Some(line) = log {
            self.push_log(line);
        }
        cx.notify();
    }

    /// Returns whether the idle timer for `generation` should keep running.
    fn check_voice_idle(&mut self, generation: u64, cx: &mut Context<Self>) -> bool {
        let Some(voice) = self.voice.as_mut().filter(|v| v.generation == generation) else {
            return false;
        };
        if !voice.running() {
            return false;
        }
        if voice.idle_expired(Instant::now()) {
            voice.shutdown();
            self.push_log("voice sidecar unloaded after idle".into());
            cx.notify();
            return false;
        }
        true
    }

    pub(crate) fn voice_before_frame(&self, topic: &str) -> Option<VoiceBefore> {
        self.voice.as_ref()?;
        let session_id = topic.strip_prefix("conversation/")?;
        let conversation = self.conversations.get(session_id);
        Some(VoiceBefore {
            session_id: session_id.to_string(),
            first_load: !conversation.is_some_and(|c| c.subscribed),
            phase: conversation.map(|c| c.phase.clone()).unwrap_or_default(),
        })
    }

    /// Turn starts reset "spoke this turn"; newly shown prompts get a
    /// fallback line when the agent hasn't spoken this turn.
    pub(crate) fn voice_after_frame(
        &mut self,
        before: Option<VoiceBefore>,
        cx: &mut Context<Self>,
    ) {
        let Some(before) = before else { return };
        let Some(conversation) = self.conversations.get(&before.session_id) else {
            return;
        };
        let starts = rules::starts_turn(&before.phase, &conversation.phase);
        let pending = conversation.pending_interactions.clone();
        let Some(voice) = self.voice.as_mut() else {
            return;
        };
        if starts {
            voice.ledger.turn_started(&before.session_id);
        }
        let Some(kind) = voice
            .ledger
            .fallback_for(&before.session_id, &pending, before.first_load)
        else {
            return;
        };
        let settings = settings(cx);
        if rules::speak_gate(&settings, voice.window_active).is_ok() {
            let _ = self.voice_say(kind.line(), &settings, cx);
        }
    }
}
