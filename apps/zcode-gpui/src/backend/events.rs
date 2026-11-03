//! Connection event handling: pumps per workspace agent (with producer-side
//! backpressure signaling), wire routing, fault recovery (resync) and
//! auto-restart. Frame application lives in backend/route.rs; response
//! correlation lives in backend/responses.rs.

use crate::app::store::AppState;
use crate::backend::backlog::EventBacklog;
use crate::backend::launcher::ConnEvent;
use futures::StreamExt;
use gpui::Context;
use serde_json::Value;

/// Producer-side backlog at which we tell the CLI to stop streaming frames:
/// either watermark triggers the pause (follow-up review finding 6 —
/// count-only flow control let near-1 MiB frames hit the byte cap after
/// ~60 events, long before the count pause).
const FLOW_HIGH: usize = 2000;
const FLOW_HIGH_BYTES: usize = 32 * 1024 * 1024;
/// Backlog at which streaming may resume (both watermarks below the low line).
const FLOW_LOW: usize = 200;
const FLOW_LOW_BYTES: usize = 4 * 1024 * 1024;
/// Consecutive agent crashes before we give up and ask the user to reconnect.
const MAX_RESTARTS: u32 = 3;

/// Attach the event pump for a workspace's agent connection. Backlog is
/// counted on the producer side (each enqueued event), so the flow signal
/// reflects real queue depth even when the serial consumer lags behind
/// (2026-10-05 audit P0.3). Protocol overflow (dropped stdout lines) is a
/// desynchronized connection: the pump restarts it instead of resyncing
/// routes, because route resync cannot repair lost control envelopes.
pub(crate) fn attach_pump(
    cx: &mut Context<AppState>,
    ws_key: String,
    generation: u64,
    backlog: EventBacklog,
    log_backlog: EventBacklog,
    mut events: futures::channel::mpsc::UnboundedReceiver<ConnEvent>,
) {
    cx.spawn(async move |this, cx| {
        while let Some(ev) = events.next().await {
            let current = this
                .update(cx, |s, _| {
                    s.ws(&ws_key).is_some_and(|w| w.generation == generation)
                })
                .unwrap_or(false);
            if !current {
                break;
            }
            // Depth includes everything the producer queued and we have not
            // released yet — the true producer backlog.
            if backlog.depth() >= FLOW_HIGH || backlog.bytes() >= FLOW_HIGH_BYTES {
                let key = ws_key.clone();
                let _ = this.update(cx, |s, _| {
                    if s.ws(&key).is_some_and(|w| w.generation == generation) {
                        s.flow_latch(&key, true)
                    }
                });
            }
            let len = event_len(&ev);
            let is_exit = matches!(ev, ConnEvent::Exited);
            let is_log = matches!(ev, ConnEvent::Log(_));
            let keep_going = this
                .update(cx, |state, cx| {
                    if state
                        .ws(&ws_key)
                        .is_some_and(|w| w.generation == generation)
                    {
                        state.handle_conn(&ws_key, ev, cx)
                    } else {
                        false
                    }
                })
                .unwrap_or(false);
            // Exited bypassed producer admission, so it is never released;
            // logs release against the independent stderr backlog.
            let depth = if is_exit {
                backlog.depth()
            } else if is_log {
                log_backlog.release(len)
            } else {
                backlog.release(len)
            };
            if depth <= FLOW_LOW && backlog.bytes() <= FLOW_LOW_BYTES {
                let key = ws_key.clone();
                let _ = this.update(cx, |s, _| {
                    if s.ws(&key).is_some_and(|w| w.generation == generation) {
                        s.flow_latch(&key, false)
                    }
                });
            }
            if backlog.take_overflow() && keep_going {
                let key = ws_key.clone();
                let _ = this.update(cx, |state, cx| {
                    if state.ws(&key).is_some_and(|w| w.generation == generation) {
                        state.restart_connection(&key, cx)
                    }
                });
            }
            if !keep_going {
                break;
            }
        }
    })
    .detach();
}

fn event_len(ev: &ConnEvent) -> usize {
    match ev {
        ConnEvent::Line(l) | ConnEvent::Log(l) => l.len(),
        ConnEvent::Exited => 0,
    }
}

impl AppState {
    pub(crate) fn handle_conn(
        &mut self,
        ws_key: &str,
        ev: ConnEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        match ev {
            ConnEvent::Line(line) => self.handle_line(ws_key, line, cx),
            ConnEvent::Log(l) => {
                // Agent stderr can carry provider request dumps; scrub before
                // it reaches the console or the in-memory log.
                let l = crate::shared::redact::scrub(&l);
                eprintln!("[zcode-gpui:{ws_key}:stderr] {l}");
                self.push_log(l);
                cx.notify();
                true
            }
            ConnEvent::Exited => self.handle_exit(ws_key, cx),
        }
    }

    /// Agent process died. An idle unload is expected (no respawn); otherwise
    /// auto-restart with linear backoff up to MAX_RESTARTS, then surface a
    /// reconnect error. Conversation mirrors survive — the fresh snapshot
    /// delivered on re-subscribe rebuilds them.
    fn handle_exit(&mut self, ws_key: &str, cx: &mut Context<Self>) -> bool {
        let Some(ws) = self.ws_mut(ws_key) else {
            return false;
        };
        ws.kill = None;
        if ws.status.starts_with("idle") {
            // Deliberate unload: no respawn, the pump ends here.
            ws.pumping = false;
            cx.notify();
            return true;
        }
        if ws.remote.is_some() {
            return self.handle_remote_exit(ws_key, cx);
        }
        if !ws.started {
            // Candidate failed before storage startup: next candidate,
            // otherwise give up on this workspace.
            ws.inbound = None;
            let display = ws.display.clone();
            match ws.try_spawn() {
                Some(events) => {
                    eprintln!(
                        "[zcode-gpui] backend for {ws_key} exited before startup, trying next candidate"
                    );
                    self.push_log(format!(
                        "backend for {ws_key} died before startup, trying next candidate"
                    ));
                    if let Some((backlog, log_backlog)) = self
                        .ws(ws_key)
                        .and_then(|w| Some((w.backlog.clone()?, w.log_backlog.clone()?)))
                    {
                        let generation = self.ws(ws_key).map_or(0, |w| w.generation);
                        attach_pump(
                            cx,
                            ws_key.to_string(),
                            generation,
                            backlog,
                            log_backlog,
                            events,
                        );
                    }
                }
                None => {
                    ws.status = "no backend".into();
                    eprintln!("[zcode-gpui] no usable backend for {display}");
                    self.push_error(format!(
                        "no usable backend for {display} — install bun (or ship zcode-backend beside GPUI)"
                    ));
                }
            }
            cx.notify();
            return true;
        }
        self.reset_connection_state(ws_key);
        let attempts = self.ws(ws_key).map(|w| w.restart_attempts).unwrap_or(0);
        if attempts >= MAX_RESTARTS {
            if let Some(ws) = self.ws_mut(ws_key) {
                ws.status = "backend down".into();
            }
            self.push_error(format!(
                "agent for {} keeps exiting — click the project to reconnect",
                self.ws(ws_key)
                    .map(|w| w.display.clone())
                    .unwrap_or_default()
            ));
            cx.notify();
            return true;
        }
        let delay_ms = 1000u64 * (attempts as u64 + 1);
        if let Some(ws) = self.ws_mut(ws_key) {
            ws.restart_attempts += 1;
            ws.status = format!("restarting (attempt {}/{})", attempts + 1, MAX_RESTARTS);
        }
        self.push_log(format!(
            "agent for {ws_key} exited; restarting in {delay_ms}ms"
        ));
        let key = ws_key.to_string();
        let generation = self.ws(ws_key).map_or(0, |w| w.generation);
        cx.spawn(async move |this, cx| {
            // 重启等待用 executor timer，避免占用 IO worker，并可确定性验证旧代失效。
            cx.background_executor()
                .timer(std::time::Duration::from_millis(delay_ms))
                .await;
            let _ = this.update(cx, |state, cx| {
                if state
                    .ws(&key)
                    .is_some_and(|w| w.generation == generation && !w.pumping)
                {
                    state.spawn_workspace(&key, cx)
                }
            });
        })
        .detach();
        cx.notify();
        true
    }

    /// Clear per-connection mirrors so a fresh subscribe rebuilds them:
    /// pending rpcs, subscription ids, route cursors, fragment state. Topic
    /// cleanup goes through the shared transport-state reset (follow-up
    /// review finding 2) so child and non-indexed routes die with the
    /// connection too.
    pub(crate) fn reset_connection_state(&mut self, ws_key: &str) {
        if let Some(ws) = self.ws_mut(ws_key) {
            ws.invalidate_connection();
            ws.inbound = None;
            ws.started = false;
            ws.pumping = false; // the old pump is ending; allow respawn
            ws.prepare_respawn();
            ws.pending.clear();
            ws.desired_conversation = None;
        }
        self.clear_workspace_transport_state(ws_key);
        self.flow_saturated.remove(ws_key);
    }

    fn handle_line(&mut self, ws_key: &str, line: String, cx: &mut Context<Self>) -> bool {
        if line.len() > crate::backend::backlog::MAX_RAW_LINE_BYTES {
            self.push_log(format!("dropped oversized line ({} bytes)", line.len()));
            return true;
        }
        // Phone-owned responses and frames never reach the desktop's own
        // correlation or frame assembler.
        if self.route_cli_line_to_phone(ws_key, &line) {
            return true;
        }
        let Some(incoming) = crate::backend::wire::parse_line(&line) else {
            self.push_log(format!("unparsable stdout line: {line:.120}"));
            return true;
        };
        match incoming {
            crate::backend::wire::Incoming::AgentRequest { id, method, params } => {
                if self.route_browser_rpc(ws_key, &id, &method, &params)
                    || self.route_voice_rpc(ws_key, &id, &method, &params, cx)
                {
                    return true;
                }
                // 未提交或失败的偏好不能影响运行时；仅从唯一 owner 的已提交快照读取。
                let memory_enabled = cx
                    .try_global::<crate::shared::preferences::PreferenceOwner>()
                    .is_some_and(|owner| owner.0.read(cx).snapshot.memory_enabled.unwrap_or(false));
                let prefs = crate::backend::reverse_rpc::HostPrefs {
                    memory_enabled,
                    voice_enabled: self.voice.is_some() && crate::voice::settings(cx).enabled,
                    code_search_enabled: cx
                        .try_global::<crate::shared::preferences::PreferenceOwner>()
                        .is_none_or(|owner| owner.0.read(cx).snapshot.code_search()),
                    lsp_diagnostics_enabled: cx
                        .try_global::<crate::shared::preferences::PreferenceOwner>()
                        .is_none_or(|owner| owner.0.read(cx).snapshot.lsp_diagnostics()),
                    tool_search_mode: cx
                        .try_global::<crate::shared::preferences::PreferenceOwner>()
                        .map_or("off", |owner| owner.0.read(cx).snapshot.tool_search_mode()),
                    prompt_cache_modes: cx
                        .try_global::<crate::shared::preferences::PreferenceOwner>()
                        .map_or(serde_json::json!({}), |owner| {
                            owner.0.read(cx).snapshot.prompt_cache_modes_json()
                        }),
                };
                let action =
                    crate::backend::reverse_rpc::dispatch_reverse_rpc(&id, &method, &params, prefs);
                match action {
                    crate::backend::reverse_rpc::ReverseRpcAction::Raced => true,
                    crate::backend::reverse_rpc::ReverseRpcAction::Respond(resp) => {
                        if let Some(ws) = self.ws_mut(ws_key) {
                            ws.send_line(resp);
                        }
                        true
                    }
                }
            }
            crate::backend::wire::Incoming::Notification { method, params } => {
                match method.as_str() {
                    "startup/storageState" => {
                        let phase = params.get("phase").and_then(Value::as_str).unwrap_or("");
                        if phase == "ready" {
                            let already = self.ws(ws_key).map(|w| w.started).unwrap_or(true);
                            if !already {
                                if let Some(ws) = self.ws_mut(ws_key) {
                                    ws.started = true;
                                    ws.status = "connected".into();
                                    ws.restart_attempts = 0;
                                    ws.last_activity = Some(std::time::Instant::now());
                                }
                                self.subscribe_index(ws_key);
                                self.subscribe_config(ws_key);
                                self.probe_model_catalog(ws_key);
                                self.sync_system_prompt(ws_key);
                                self.sync_secrets(ws_key);
                                // Cold-open: the user already picked a session
                                // while this agent was starting.
                                let desired = self
                                    .ws_mut(ws_key)
                                    .and_then(|w| w.desired_conversation.take());
                                if let Some(sid) = desired {
                                    self.subscribe_conversation(ws_key, &sid);
                                }
                                self.phone_workspace_ready(ws_key);
                            }
                        }
                    }
                    "v4/conversation/frame" => {
                        // Surface timed-out assemblies BEFORE ingesting: the
                        // sweep inside ingest used to discard the topics, so
                        // an expiration performed by an unrelated incoming
                        // frame was never resynced (review finding 5).
                        for topic in self.assembler.sweep_timeouts() {
                            self.push_log(format!("assembly timed out on {topic}"));
                            if let Some(k) = self.ws_for_topic(&topic) {
                                self.resync_topic(&k, &topic);
                            }
                        }
                        match self.assembler.ingest(&params) {
                            Ok(Some(frame)) => self.route_frame(frame, cx),
                            Ok(None) => {}
                            Err(e) => {
                                // Corrupt frame (CRC/schema/ordinal conflict):
                                // request a fresh snapshot for that route.
                                let topic = params
                                    .get("topic")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string();
                                self.push_log(format!("frame error: {e}"));
                                self.resync_topic(ws_key, &topic);
                            }
                        }
                    }
                    _ => {}
                }
                true
            }
            crate::backend::wire::Incoming::Response { id, result, error } => {
                self.handle_response(ws_key, id, result, error, cx);
                true
            }
        }
    }
}
