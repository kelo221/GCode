//! Session-scoped V4 commands: one envelope builder, CAS fields filled from
//! the mirrored conversation, and ack handling (stale retry, failure rollback).
//!
//! Spec source: packages/shared/src/zcode-protocol-v4/command.ts
//! (`commandEnvelopeSchema`, `COMMANDS_REQUIRING_BASE_REVISION`,
//! `ROW_TARGETING_COMMANDS`, `commandAckSchema`).
//!
//! Ownership: the backend is the only source of truth. Optimistic UI updates
//! are rolled back by re-fetching the authoritative snapshot (forced resync)
//! instead of keeping a second, client-side undo path.

use crate::app::store::AppState;
use crate::backend::workspace::{CommandCtx, Pending};
use serde_json::{Value, json};

/// `v4/command` params (the CommandEnvelope). Pure so golden tests can pin
/// the wire shape of every command.
#[allow(clippy::too_many_arguments)]
pub(crate) fn command_params(
    client_id: &str,
    sid: Option<&str>,
    ctype: &str,
    payload: Value,
    base_revision: Option<u64>,
    base_log_epoch: Option<&str>,
    command_id: String,
    issued_at: u64,
) -> Value {
    let mut params = json!({
        "commandId": command_id,
        "clientId": client_id,
        "sessionId": sid,
        "type": ctype,
        "payload": payload,
        "issuedAt": issued_at,
    });
    if let Some(base) = base_revision {
        params["baseRevision"] = json!(base);
    }
    if let Some(epoch) = base_log_epoch {
        params["baseLogEpoch"] = json!(epoch);
    }
    params
}

/// `COMMANDS_REQUIRING_BASE_REVISION` (command.ts). The backend rejects these
/// without `baseRevision`, so the client enforces it regardless of call site.
pub(crate) const COMMANDS_REQUIRING_BASE_REVISION: &[&str] = &[
    "applyFileRewind",
    "forkAssistant",
    "editUserQuery",
    "retryTurn",
    "setAssistantFeedback",
    "sendQueuedNow",
    "editQueueItem",
    "reorderQueueItem",
    "deleteQueueItem",
    "setAutoDrain",
    "switchModelConfig",
    "switchCollaborationMode",
    "setFollowupMode",
    "pauseGoal",
    "resumeGoal",
];

/// `ROW_TARGETING_COMMANDS` (command.ts): also need `baseLogEpoch`.
pub(crate) const ROW_TARGETING_COMMANDS: &[&str] = &[
    "applyFileRewind",
    "forkAssistant",
    "editUserQuery",
    "retryTurn",
    "setAssistantFeedback",
];

/// Why a CAS command cannot be sent yet.
#[derive(Debug, PartialEq)]
pub(crate) enum CasBlock {
    /// No snapshot revision has been mirrored for the session.
    NoRevision,
    /// Row-targeting command without the snapshot `logEpoch`.
    NoLogEpoch,
}

/// Resolve the envelope's `(baseRevision, baseLogEpoch)`. Sources, in order:
/// the stale-retry `revisionAtDecision`, then the mirrored snapshot revision.
/// Nothing is ever defaulted or guessed; a missing source blocks the command.
pub(crate) fn cas_fields<'a>(
    ctype: &str,
    cas: bool,
    retry_base: Option<u64>,
    mirrored: Option<u64>,
    log_epoch: Option<&'a str>,
) -> Result<(Option<u64>, Option<&'a str>), CasBlock> {
    let row = ROW_TARGETING_COMMANDS.contains(&ctype);
    let needs_base = cas || row || COMMANDS_REQUIRING_BASE_REVISION.contains(&ctype);
    let epoch = log_epoch.filter(|e| !e.trim().is_empty());
    if row && epoch.is_none() {
        return Err(CasBlock::NoLogEpoch);
    }
    if !needs_base {
        return Ok((None, None));
    }
    let base = retry_base.or(mirrored).ok_or(CasBlock::NoRevision)?;
    Ok((Some(base), if row { epoch } else { None }))
}

/// What to do with a command ack (`commandAckSchema.status`).
#[derive(Debug, PartialEq)]
pub(crate) enum AckAction {
    /// accepted / noop / duplicate.
    Settled,
    /// First `stale` of a CAS command: resend from `revisionAtDecision`.
    Retry,
    /// rejected / failed / repeated stale: report and roll back.
    Fail,
}

pub(crate) fn ack_action(status: &str, cas: bool, retried: bool) -> AckAction {
    match status {
        "accepted" | "noop" | "duplicate" => AckAction::Settled,
        "stale" if cas && !retried => AckAction::Retry,
        _ => AckAction::Fail,
    }
}

/// Row-targeting payload: `{ target: { rowId, entityId }, ...extra }`.
pub(crate) fn row_payload(row_id: u64, entity_id: &str, extra: Value) -> Value {
    let mut payload = json!({ "target": { "rowId": row_id, "entityId": entity_id } });
    if let (Some(obj), Value::Object(more)) = (payload.as_object_mut(), extra) {
        obj.extend(more);
    }
    payload
}

/// Human label for error banners.
fn command_label(ctype: &str) -> &str {
    match ctype {
        "editUserQuery" => "Edit message",
        "retryTurn" => "Retry",
        "applyFileRewind" => "Undo file changes",
        "forkAssistant" => "Fork branch",
        "setAssistantFeedback" => "Feedback",
        "renameSession" => "Rename session",
        "deleteSession" => "Delete session",
        "sendQueuedNow" => "Send queued message",
        "editQueueItem" => "Edit queued message",
        "reorderQueueItem" => "Reorder queue",
        "deleteQueueItem" => "Remove queued message",
        "setAutoDrain" => "Pause/resume queue",
        "cancelBackgroundWork" => "Cancel task",
        "stopSubagent" => "Stop subagent",
        "compact" => "Compact session",
        "sendGoalCommand" => "Send goal",
        "resolveInteraction" => "Answer",
        "switchModelConfig" => "Model switch",
        "switchCollaborationMode" => "Mode switch",
        other => other,
    }
}

impl AppState {
    /// Send a session command. CAS commands take `baseRevision` from the
    /// mirrored conversation; there is no fallback revision, because a guessed
    /// base can only ever come back `stale`.
    pub(crate) fn send_session_command(&mut self, ws_key: &str, ctx: CommandCtx) -> bool {
        let mirrored = self
            .conversations
            .get(&ctx.sid)
            .filter(|c| c.revision_known)
            .map(|c| c.revision);
        let fields = cas_fields(
            &ctx.ctype,
            ctx.cas,
            ctx.retry_base,
            mirrored,
            ctx.log_epoch.as_deref(),
        );
        let (base_revision, base_log_epoch) = match fields {
            Ok(f) => f,
            Err(_) => {
                self.push_error(format!(
                    "{}: conversation is still loading; try again in a moment",
                    command_label(&ctx.ctype)
                ));
                return false;
            }
        };
        let params = command_params(
            &self.client_id,
            Some(&ctx.sid),
            &ctx.ctype,
            ctx.payload.clone(),
            base_revision,
            base_log_epoch,
            crate::backend::launcher::new_command_id(),
            crate::backend::launcher::now_ms(),
        );
        self.send_envelope(ws_key, params, Pending::Command(ctx))
    }

    /// Row-targeting commands need the snapshot's `logEpoch`; without it the
    /// action is unavailable rather than sent with a guessed epoch.
    pub(crate) fn row_epoch(&mut self, sid: &str) -> Option<String> {
        let epoch = self
            .conversations
            .get(sid)
            .and_then(|c| c.log_epoch.clone());
        if epoch.is_none() {
            self.push_error("Conversation is still loading; try again in a moment".into());
        }
        epoch
    }

    pub(crate) fn handle_command_ack(
        &mut self,
        ws_key: &str,
        ack: Option<&Value>,
        ctx: CommandCtx,
    ) {
        let field = |k: &str| ack.and_then(|a| a.get(k)).and_then(Value::as_str);
        // A malformed ack (no status) is not evidence of acceptance.
        let status = field("status").unwrap_or("malformed ack").to_string();
        let revision = ack
            .and_then(|a| a.get("revisionAtDecision"))
            .and_then(Value::as_u64);
        let cas = ctx.cas || COMMANDS_REQUIRING_BASE_REVISION.contains(&ctx.ctype.as_str());
        let action = match ack_action(&status, cas, ctx.retried) {
            // A stale ack without `revisionAtDecision` gives nothing to retry from.
            AckAction::Retry if revision.is_none() => AckAction::Fail,
            a => a,
        };
        match action {
            // The mirrored revision is not bumped here: it advances only from
            // the snapshot / `state.updated` stream, never by local arithmetic.
            AckAction::Settled => {
                self.check_result(ack.and_then(|a| a.get("result")), &ctx);
            }
            // Revision drifted (internal backend events bump it between
            // snapshots). Retry once from the server's decision revision with a
            // new commandId; the log epoch still guards row identity.
            AckAction::Retry => {
                let retry = CommandCtx {
                    retried: true,
                    retry_base: revision,
                    ..ctx
                };
                self.send_session_command(ws_key, retry);
            }
            AckAction::Fail => {
                let reason = field("message").or(field("reasonCode")).unwrap_or("");
                let raw = format!(
                    "{} {status}{}{reason}",
                    command_label(&ctx.ctype),
                    if reason.is_empty() { "" } else { ": " }
                );
                // 有明确补救的 reasonCode（模型不可用、子代理停止被拒）显示说明，原始行进日志。
                let banner = crate::backend::ack_reasons::ack_reason_banner(ack);
                if banner.is_some() {
                    self.push_log(raw.clone());
                }
                self.push_error(banner.unwrap_or(raw));
                self.rollback_command(ws_key, &ctx);
            }
        }
    }

    /// Accepted acks can still report that nothing happened.
    fn check_result(&mut self, result: Option<&Value>, ctx: &CommandCtx) {
        let Some(result) = result else { return };
        let label = command_label(&ctx.ctype);
        if ctx.ctype == "applyFileRewind"
            && result.get("applied").and_then(Value::as_bool) == Some(false)
        {
            let why = result.get("response").and_then(Value::as_str).unwrap_or("");
            self.push_error(format!("{label} not applied: {why}"));
        }
        if ctx.ctype == "editUserQuery"
            && result.get("disposition").and_then(Value::as_str) == Some("blocked")
        {
            let why = result
                .get("reasonCode")
                .and_then(Value::as_str)
                .unwrap_or("blocked");
            self.push_error(format!("{label} blocked: {why}"));
        }
        if ctx.ctype == "forkAssistant"
            && let Some(new_sid) = result.get("sessionId").and_then(Value::as_str)
        {
            let sid = new_sid.to_string();
            self.draft = false;
            self.active = Some(sid.clone());
            self.push_log(format!("forked session: {sid}"));
            let key = self.active_ws_key().unwrap_or_default();
            if !key.is_empty() {
                self.subscribe_conversation(&key, &sid);
            }
        }
    }

    /// Re-fetch the authoritative state an optimistic update diverged from.
    pub(crate) fn rollback_command(&mut self, ws_key: &str, ctx: &CommandCtx) {
        // A failed answer frees the interaction so the resynced card is
        // answerable again.
        if ctx.ctype == "resolveInteraction"
            && let Some(iid) = ctx.payload.get("interactionId").and_then(Value::as_str)
            && let Some(c) = self.conversations.get_mut(&ctx.sid)
        {
            c.resolving.release(iid);
        }
        let topic = match ctx.ctype.as_str() {
            "deleteSession" | "renameSession" => format!("sessions-index/{ws_key}"),
            _ => format!("conversation/{}", ctx.sid),
        };
        self.resync_topic(ws_key, &topic);
    }
}

#[cfg(test)]
#[path = "session_cmds_tests.rs"]
mod tests;
