//! Response correlation: pending-entry resolution, subscription-id capture, CAS config acks (stale retry), history pages.

use crate::app::store::AppState;
use crate::backend::workspace::Pending;
use gpui::Context;
use serde_json::Value;

impl AppState {
    pub(crate) fn handle_response(
        &mut self,
        ws_key: &str,
        id: u64,
        result: Option<Value>,
        error: Option<Value>,
        cx: &mut Context<Self>,
    ) {
        if self.handle_workflow_content_response(ws_key, id, &result, &error, cx)
            || self.handle_workflow_artifact_response(ws_key, id, &result, &error, cx)
            || self.handle_attachment_response(ws_key, id, &result, &error, cx)
            || self.handle_raw_query_response(ws_key, id, &result, &error, cx)
            || self.handle_system_prompt_response(ws_key, id, &result, &error, cx)
            || self.handle_secrets_response(ws_key, id, &error, cx)
            || self.handle_provider_settings_response(ws_key, id, &result, &error, cx)
            || self.handle_auto_compact_response(ws_key, id, &result, &error)
        {
            return;
        }
        if let Some(err) = error {
            // 失效请求的迟到错误没有 owner；不能污染新页面或回显已清除请求中的密钥。
            if !self.ws(ws_key).is_some_and(|w| w.pending.contains_key(&id)) {
                return;
            }
            // Background freshness probes fail silently: the next tick retries,
            // and a transient store hiccup must not banner the user.
            if let Some(ws) = self.ws_mut(ws_key)
                && matches!(ws.pending.get(&id), Some(Pending::PollRows(_)))
            {
                ws.pending.remove(&id);
                cx.notify();
                return;
            }
            let msg = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("rpc error");
            // 手机端会自动查询子智能体、斜杠命令等桌面专属面板；桌面网关按白名单拒绝。
            // 这类拒绝是预期行为：只结算 pending 让面板显示不可用，不弹错误横幅也不标记工作区出错。
            let refused = crate::remote::scope::is_phone_scope_refusal(&err);
            let msg = if self
                .ws(ws_key)
                .is_some_and(|w| matches!(w.pending.get(&id), Some(Pending::SlashCatalog(_))))
            {
                crate::composer::slash_catalog::SLASH_CATALOG_ERROR
            } else if self
                .ws(ws_key)
                .is_some_and(|w| matches!(w.pending.get(&id), Some(Pending::FetchMcpList)))
            {
                crate::shared::mcp::MCP_ERROR
            } else {
                self.plugin_config_feedback(ws_key, id, msg)
            };
            if !refused {
                self.status_error(ws_key, msg);
                self.push_error(format!("request failed: {msg}"));
            }
            let pending = self.ws_mut(ws_key).and_then(|ws| ws.pending.remove(&id));
            if let Some(pending) = &pending {
                self.settle_inspection_error(ws_key, pending, msg);
            }
            match pending {
                Some(Pending::SlashCatalog(session)) => {
                    self.settle_slash_catalog(ws_key, session, None)
                }
                Some(Pending::ReferenceCatalog { session, kind }) => {
                    self.settle_reference_catalog(ws_key, session, kind, Err(msg.to_string()));
                }
                Some(Pending::SavedWorkflowList(scope)) => {
                    self.settle_saved_workflow_list(ws_key, &scope, Err(msg.into()))
                }
                Some(Pending::SavedWorkflowCreate(launch)) => {
                    self.saved_workflow_launch_error(ws_key, &launch, msg)
                }
                Some(Pending::SavedWorkflowStart { session, launch }) => {
                    self.cleanup_workflow_target(ws_key, &session);
                    self.saved_workflow_launch_error(ws_key, &launch, msg);
                }
                Some(Pending::PluginPrompt(_)) => self.plugin_prompt_error(ws_key, msg),
                Some(Pending::PluginConfig(request)) => {
                    self.settle_plugin_config(ws_key, request, Err(msg.into()), cx)
                }
                Some(Pending::PluginDescribe(identity)) => {
                    self.settle_plugin_description(ws_key, &identity, Err(msg.into()))
                }
                Some(Pending::WorkflowDefinition { scope, name }) => {
                    self.settle_workflow_definition(ws_key, &scope, &name, Err(msg.into()))
                }
                Some(Pending::WorkflowHistory { scope, name }) => {
                    self.settle_workflow_history(ws_key, &scope, &name, Err(msg.into()))
                }
                Some(Pending::WorkflowPreflight(receipt)) => {
                    self.workflow_management_error(ws_key, &receipt, msg)
                }
                Some(Pending::WorkflowMutation(receipt)) => {
                    self.settle_workflow_mutation(ws_key, receipt, Err(msg.into()), cx)
                }
                Some(Pending::WorkflowSettings { session, run }) => {
                    self.workflow_settings_error(ws_key, &session, &run, msg)
                }
                Some(Pending::SavedWorkflowCleanup) => {
                    self.saved_workflow_error(ws_key, "Workflow target cleanup failed")
                }
                Some(Pending::QueueEdit(restore)) => self.settle_queue_edit(restore, None, cx),
                Some(Pending::SubagentDirectory { session, replacing }) => {
                    self.settle_subagent_directory(ws_key, &session, replacing, Err(msg.into()), cx)
                }
                Some(Pending::Command(ctx)) => self.rollback_command(ws_key, &ctx),
                Some(
                    Pending::SendText(submission)
                    | Pending::CreateSession(submission)
                    | Pending::HeldSend { submission, .. },
                ) => self.recover_submission(submission, cx),
                _ => {}
            }
            cx.notify();
            return;
        }
        // Take the pending entry (workspace borrow must end before we mutate
        // conversations below).
        let Some(pending) = self.ws_mut(ws_key).and_then(|ws| ws.pending.remove(&id)) else {
            return;
        };
        match pending {
            Pending::SubscribeIndex => {
                self.capture_subscription(ws_key, "sessions-index", &result);
                self.push_log("sessions-index subscribed".into());
            }
            Pending::SubscribeConfig => {
                self.capture_subscription(ws_key, "workspace-config", &result);
            }
            Pending::ModelCatalog => {
                // Harvest the model catalog from the legacy session/create
                // settings snapshot, then delete the throwaway session it
                // persisted (keeps the sidebar clean).
                let legacy_sid = result
                    .as_ref()
                    .and_then(|r| r.get("sessionId"))
                    .or_else(|| {
                        result
                            .as_ref()
                            .and_then(|r| r.pointer("/session/sessionId"))
                    })
                    .or_else(|| result.as_ref().and_then(|r| r.pointer("/record/sessionId")))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let ws_key = ws_key.to_string();
                if let Some(settings) = result.as_ref().and_then(|r| r.get("settings")) {
                    self.cache_model_settings(&ws_key, settings);
                    self.workspace_configs
                        .entry(ws_key.clone())
                        .or_default()
                        .apply_settings(settings);
                    self.push_log(format!(
                        "model catalog: {} models",
                        self.workspace_configs
                            .get(&ws_key)
                            .map(|c| c.models.len())
                            .unwrap_or(0)
                    ));
                }
                if let Some(sid) = legacy_sid {
                    self.delete_session(&ws_key, &sid);
                }
            }
            Pending::AttachmentUpload { .. }
            | Pending::AttachmentAbort
            | Pending::WorkflowArtifacts(_)
            | Pending::WorkflowArtifactContent(_)
            | Pending::SystemPrompt { .. }
            | Pending::Secrets
            | Pending::ProviderSettings(_)
            | Pending::AutoCompact(_)
            | Pending::RawQuery(_)
            | Pending::CleanupCatalog => {}
            Pending::SubscribeConversation(sid) => {
                let topic = format!("conversation/{sid}");
                self.capture_subscription(ws_key, &topic, &result);
                let c = self.conversations.entry(sid.clone()).or_default();
                c.subscribed = true;
                self.push_log(format!("subscribed conversation {sid}"));
            }
            Pending::CreateSession(submission) => {
                self.handle_submission_ack(ws_key, submission, result.as_ref(), true, cx);
            }
            // The CAS revision is mirrored only from the conversation stream.
            Pending::SendText(submission) => {
                self.handle_submission_ack(ws_key, submission, result.as_ref(), false, cx);
            }
            Pending::HeldSend {
                submission,
                trigger,
            } => self.settle_held_submission(ws_key, submission, trigger, result.as_ref(), cx),
            Pending::QueueEdit(restore) => self.settle_queue_edit(restore, result.as_ref(), cx),
            Pending::SubagentDirectory { session, replacing } => self.settle_subagent_directory(
                ws_key,
                &session,
                replacing,
                result.ok_or_else(|| "Missing subagent directory response".into()),
                cx,
            ),
            Pending::Stop => {
                if !crate::backend::submission::ack_succeeded(result.as_ref()) {
                    self.push_error("Stop was not accepted".into());
                }
            }
            Pending::Command(ctx) => {
                self.handle_command_ack(ws_key, result.as_ref(), ctx);
            }
            Pending::Resync(topic) => {
                // The resync result wraps a subscribe ack; reconcile the
                // active route generation BEFORE the recovery snapshot
                // arrives, or the snapshot would be rejected against the
                // stale epoch (review finding 1).
                self.capture_subscription(ws_key, &topic, &result);
                self.push_log(format!("resync acknowledged for {topic}"));
            }
            Pending::FetchRows(sid) => {
                let (rows, has_more) = match &result {
                    Some(r) => (
                        r.get("rows").and_then(Value::as_array).cloned(),
                        r.get("hasMore").and_then(Value::as_bool),
                    ),
                    None => (None, None),
                };
                let count = rows.as_ref().map(|r| r.len()).unwrap_or(0);
                if let Some(c) = self.conversations.get_mut(&sid) {
                    c.apply_older_rows(rows.as_deref().unwrap_or_default(), has_more);
                    // `atRevision` describes when the page was read; it can be
                    // older than the live mirror, so it never overwrites it.
                }
                self.push_log(format!("history page: +{count} rows"));
            }
            Pending::PollRows(sid) => {
                // Change probe, never applied. The poll makes the backend
                // refresh its cold projection when the store was appended by
                // another process; movement here triggers the standard resync
                // whose snapshot repaints everything (rows, statuses, plan,
                // queue) through the §6 pipeline.
                let Some(r) = &result else { return };
                let at_seq = r.get("atSeq").and_then(Value::as_u64).unwrap_or(0);
                let at_epoch = r
                    .get("atLogEpoch")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let max_row_id = r.get("rows").and_then(Value::as_array).and_then(|rows| {
                    rows.iter()
                        .filter_map(|v| v.get("rowId").and_then(Value::as_u64))
                        .max()
                });
                let Some(c) = self.conversations.get_mut(&sid) else {
                    return;
                };
                // If the mirror's seq advanced since the previous poll, our own
                // deltas are flowing (we host the turn) — the atSeq comparison
                // would false-positive on every in-flight frame and is skipped.
                let deltas_flowing = c.seq != c.prev_poll_seq;
                c.prev_poll_seq = c.seq;
                let newer_row = max_row_id
                    .is_some_and(|id| c.rows.last_key_value().is_none_or(|(k, _)| id > *k));
                let epoch_moved = at_epoch.is_some_and(|e| c.log_epoch.as_ref() != Some(&e));
                let seq_moved = !deltas_flowing && at_seq != 0 && at_seq != c.seq;
                if newer_row || epoch_moved || seq_moved {
                    self.push_log(format!(
                        "poll detected movement (newer_row={newer_row}, epoch={epoch_moved}, seq={seq_moved}) -> resync"
                    ));
                    let topic = format!("conversation/{sid}");
                    self.resync_topic(ws_key, &topic);
                }
            }
            Pending::FetchUsageStats(range) => {
                if let Some(res) = &result {
                    if let Some(ws) = self.ws_mut(ws_key) {
                        ws.inspection
                            .usage
                            .entry(range.clone())
                            .or_default()
                            .finish(crate::shared::usage_stats::AppUsageSnapshot::from_value(
                                res, &range,
                            ));
                    }
                    self.push_log(format!("usage stats loaded ({range})"));
                } else if let Some(ws) = self.ws_mut(ws_key) {
                    ws.inspection
                        .usage
                        .entry(range)
                        .or_default()
                        .fail("Missing usage response");
                }
            }
            Pending::SlashCatalog(session) => self.settle_slash_catalog(ws_key, session, result),
            Pending::ReferenceCatalog { session, kind } => {
                self.settle_reference_catalog(
                    ws_key,
                    session,
                    kind,
                    result.ok_or_else(|| "Missing reference catalog response".to_string()),
                );
            }
            Pending::FetchMcpList => {
                if let Some(ws) = self.ws_mut(ws_key) {
                    match result {
                        Some(res) => {
                            match crate::shared::mcp::McpServerSnapshot::list_from_value(&res) {
                                Ok(list) => ws.inspection.mcp.finish(list),
                                Err(error) => ws.inspection.mcp.fail(&error),
                            }
                        }
                        None => ws.inspection.mcp.fail("Missing MCP response"),
                    }
                }
            }
            Pending::FetchPluginsOverview => {
                if let Some(ws) = self.ws_mut(ws_key) {
                    match result {
                        Some(res) => match crate::shared::plugin_results::validate_overview(&res) {
                            Ok(()) => ws.inspection.plugins.finish(
                                crate::shared::plugins::PluginsOverviewResult::from_value(&res),
                            ),
                            Err(error) => ws.inspection.plugins.fail(&error),
                        },
                        None => ws.inspection.plugins.fail("Missing plugin response"),
                    }
                }
            }
            Pending::PluginAction(desc) => {
                self.settle_plugin_action(ws_key, &desc, result.as_ref(), cx)
            }
            Pending::SavedWorkflowList(scope) => self.settle_saved_workflow_list(
                ws_key,
                &scope,
                result.ok_or_else(|| "Missing saved workflow list".into()),
            ),
            Pending::SavedWorkflowCreate(launch) => {
                self.settle_saved_workflow_create(ws_key, launch, result.as_ref(), cx)
            }
            Pending::SavedWorkflowStart { launch, session } => {
                self.settle_saved_workflow_start(ws_key, launch, &session, result.as_ref(), cx)
            }
            Pending::PluginConfig(request) => self.settle_plugin_config(
                ws_key,
                request,
                result.ok_or_else(|| "Missing plugin configuration response".into()),
                cx,
            ),
            Pending::PluginDescribe(identity) => self.settle_plugin_description(
                ws_key,
                &identity,
                result.ok_or_else(|| "Missing plugin description".into()),
            ),
            Pending::PluginPrompt(prompt) => self.settle_plugin_prompt(
                ws_key,
                prompt,
                result.ok_or_else(|| "Missing plugin reference response".into()),
                cx,
            ),
            Pending::WorkflowDefinition { scope, name } => self.settle_workflow_definition(
                ws_key,
                &scope,
                &name,
                result.ok_or_else(|| "Missing workflow definition response".into()),
            ),
            Pending::WorkflowHistory { scope, name } => self.settle_workflow_history(
                ws_key,
                &scope,
                &name,
                result.ok_or_else(|| "Missing workflow history response".into()),
            ),
            Pending::WorkflowPreflight(receipt) => self.settle_workflow_preflight(
                ws_key,
                receipt,
                result.ok_or_else(|| "Missing workflow preflight response".into()),
                cx,
            ),
            Pending::WorkflowMutation(receipt) => self.settle_workflow_mutation(
                ws_key,
                receipt,
                result.ok_or_else(|| "Missing workflow mutation response".into()),
                cx,
            ),
            Pending::WorkflowSettings { session, run } => {
                self.settle_workflow_settings(ws_key, &session, &run, result.as_ref())
            }
            Pending::SavedWorkflowCleanup => {
                if !crate::backend::submission::ack_succeeded(result.as_ref()) {
                    self.saved_workflow_error(ws_key, "Workflow target cleanup was not accepted");
                }
            }
        }
        cx.notify();
    }
}
