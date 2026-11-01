//! Conversation model parsed from V4 snapshots/deltas.
//!
//! Spec source: packages/shared/src/zcode-protocol-v4/ (rows.ts, snapshot.ts,
//! delta.ts, sessions-index.ts). Parsing is deliberately lenient: unknown row
//! kinds and delta ops are ignored so additive protocol changes don't break
//! the client (backend may be newer than this frontend).

use crate::composer::catalog::SessionConfig;
use serde_json::Value;
use std::collections::BTreeMap;

/// Single-line preview with ellipsis, for list rendering.
pub(crate) fn format_preview(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max).collect::<String>())
    } else {
        s
    }
}

pub use crate::conversation::rows::Row;
pub use crate::conversation::session_entry::SessionEntry;

/// Client mirror of one conversation topic. State patches arrive as
/// whole-key replacement (never deep-merge), per snapshot.ts/delta.ts.
#[derive(Default)]
pub struct ConversationState {
    pub rows: BTreeMap<u64, Row>,
    pub phase: String,
    pub(crate) can_stop: bool,
    pub(crate) input_routing: Option<crate::composer::delivery::InputRouting>,
    pub log_epoch: Option<String>,
    pub seq: u64,
    /// Mirrored CAS revision (`snapshot.revision`, then `state.updated`
    /// patches). Only meaningful once `revision_known` is set.
    pub revision: u64,
    /// A snapshot carrying `revision` has been applied. Until then CAS
    /// commands are unavailable: a defaulted 0 would be a guessed base.
    pub revision_known: bool,
    pub config: SessionConfig,
    pub subscribed: bool,
    /// First maintenance sweep that found this subscription releasable;
    /// cleared when it stops being releasable (`app/conversation_release.rs`).
    pub(crate) releasable_since: Option<std::time::Instant>,
    /// Oldest row of the whole conversation (wire `rows.firstRowId`). The
    /// snapshot `window` carries only the tail, so rows older than the first
    /// loaded one page in through `v4/conversation/rowsRange`.
    pub first_row_id: Option<u64>,
    pub total_count: u64,
    /// Mirror seq seen by the previous freshness probe (`Pending::PollRows`).
    /// Movement between probes means our own deltas are flowing, which
    /// suppresses the probe's seq comparison (in-flight frames would
    /// false-positive while we stream our own turn).
    pub prev_poll_seq: u64,
    /// Backend-reported failure for the current turn (control.lastError).
    pub last_error: Option<(String, String)>,
    /// Backend automatic retry state (control.apiRetry), shown verbatim.
    pub api_retry: Option<Value>,
    /// Pending interactions (permission, AskUserQuestion elicitation)
    pub pending_interactions: Vec<crate::conversation::interactions::PendingInteraction>,
    /// Answers in flight; filters re-announced cards (idempotent by id).
    pub resolving: crate::conversation::interactions::ResolvingInteractions,
    /// Active plan execution progress
    pub plan: Option<crate::conversation::turn_meta::PlanState>,
    pub(crate) goal: Option<crate::conversation::goal::GoalState>,
    pub(crate) goal_availability: crate::conversation::goal::GoalAvailability,
    pub(crate) queue_edit_allowed: bool,
    /// Follow-up input queue
    pub queue: Option<crate::conversation::queue::QueueState>,
    /// Active foreground execution id for target-scoped stop
    pub active_foreground_execution_id: Option<String>,
    /// Workflow runs state and subagents progress
    pub workflow_runs: crate::conversation::workflows::WorkflowRunsState,
    /// Mirrored subagents projection (subagentProjectionStateSchema).
    pub subagents: Option<crate::conversation::subagents::SubagentsState>,
    /// Background work items (backgroundWorkSummarySchema).
    pub background_works: Vec<crate::conversation::subagents::BackgroundWork>,
    /// Output tokens/s for the composer meter.
    pub throughput: crate::conversation::throughput::Throughput,
    /// Renderable `usage.contextWindow` for the composer ring.
    pub context_window: Option<crate::conversation::context_usage::ContextWindow>,
    /// When the phase was last seen leaving active (`conversation/auto_compact.rs`).
    pub(crate) idle_since: Option<std::time::Instant>,
    /// Auto-compacted during this idle stretch; cleared when a new run starts.
    pub(crate) auto_compacted: bool,
    pub(crate) auto_compact: Option<crate::conversation::auto_compact::AutoCompactFlight>,
    /// Terminal marker status of the last auto-compaction; the maintenance tick logs and clears it.
    pub(crate) auto_compact_outcome: Option<String>,
    /// Last row id when the last auto-compaction ended; the next one waits for newer rows.
    pub(crate) compacted_through: Option<u64>,
}

/// V4 `sessionPhaseSchema` phases during which a turn is live.
pub fn phase_is_active(phase: &str) -> bool {
    matches!(phase, "running" | "prewarming")
}

impl ConversationState {
    #[allow(dead_code)]
    pub fn running_subagents(&self) -> Vec<crate::conversation::subagents::JoinedRunningSubagent> {
        crate::conversation::subagents::join_running_subagents(
            self.subagents.as_ref(),
            &self.background_works,
        )
    }

    pub fn phase_running(&self) -> bool {
        phase_is_active(&self.phase)
    }

    fn set_pending_interactions(&mut self, pis: &[Value]) {
        let incoming = pis
            .iter()
            .filter_map(crate::conversation::interactions::PendingInteraction::from_value)
            .collect();
        self.pending_interactions = self.resolving.reconcile(incoming);
    }

    /// rowId of the oldest row held locally.
    pub fn loaded_first_row_id(&self) -> Option<u64> {
        self.rows.keys().next().copied()
    }

    /// 线协议的 `firstRowId` 是整段对话最早的行（CLI conversation-topic-publisher
    /// `getWireSnapshotForProfile`：「客户端以 window[0].rowId === firstRowId 判定已到顶」），
    /// 不是已加载窗口的起点。原先把它当窗口起点：按钮在 rowId>0 时恒显示，
    /// 点击又以 beforeRowId=最早行请求，永远拿到空页。已加载起点晚于最早行才有更早历史。
    pub fn has_more_history(&self) -> bool {
        matches!(
            (self.first_row_id, self.loaded_first_row_id()),
            (Some(oldest), Some(loaded)) if loaded > oldest
        )
    }

    pub fn apply_snapshot(&mut self, snap: &Value) {
        if let Some(epoch) = snap.get("logEpoch").and_then(Value::as_str) {
            self.log_epoch = Some(epoch.to_string());
        }
        self.seq = snap.get("seq").and_then(Value::as_u64).unwrap_or(self.seq);
        if let Some(rev) = snap.get("revision").and_then(Value::as_u64) {
            self.revision = rev;
            self.revision_known = true;
        }
        self.input_routing = snap
            .get("inputRouting")
            .and_then(crate::composer::delivery::InputRouting::from_value);
        self.can_stop = snap.pointer("/control/canStop").and_then(Value::as_bool) == Some(true);
        if let Some(control) = snap.get("control") {
            self.apply_control(control);
        }
        if let Some(config) = snap.get("config") {
            self.apply_config(config);
        }
        if let Some(usage) = snap.get("usage") {
            self.apply_usage(usage);
        }
        if let Some(pis) = snap.get("pendingInteractions").and_then(Value::as_array) {
            self.set_pending_interactions(pis);
        }
        self.queue_edit_allowed = snap
            .pointer("/availability/queueEdit/allowed")
            .and_then(Value::as_bool)
            == Some(true);
        self.goal = snap
            .get("goal")
            .and_then(crate::conversation::goal::GoalState::from_value);
        self.goal_availability = snap
            .get("availability")
            .map(crate::conversation::goal::GoalAvailability::from_value)
            .unwrap_or_default();
        if let Some(p) = snap.get("plan") {
            self.plan = crate::conversation::turn_meta::PlanState::from_value(p);
        }
        if let Some(q) = snap.get("queue") {
            self.queue = crate::conversation::queue::QueueState::from_value(q);
        }
        if let Some(wf) = snap.get("workflowRuns") {
            self.workflow_runs = crate::conversation::workflows::WorkflowRunsState::from_value(wf);
        }
        if let Some(s) = snap.get("subagents") {
            self.subagents = crate::conversation::subagents::SubagentsState::from_value(s);
        }
        if let Some(bw) = snap.get("backgroundWorks").and_then(Value::as_array) {
            self.background_works = bw
                .iter()
                .filter_map(crate::conversation::subagents::BackgroundWork::from_value)
                .collect();
        }
        if let Some(rows) = snap.get("rows") {
            if let Some(first) = rows.get("firstRowId") {
                self.first_row_id = first.as_u64();
            }
            self.total_count = rows
                .get("totalCount")
                .and_then(Value::as_u64)
                .unwrap_or(self.total_count);
            if let Some(window) = rows.get("window").and_then(Value::as_array) {
                self.rows.clear();
                for r in window {
                    let id = r.get("rowId").and_then(Value::as_u64).unwrap_or(0);
                    self.note_compact_marker(r);
                    self.rows.insert(id, Row::from_value(r));
                }
            }
        }
    }

    /// Merge a `v4/conversation/rowsRange` result (rows ascending by rowId).
    /// `has_more: Some(false)` is the server saying the oldest loaded row is
    /// the start of the conversation.
    pub fn apply_older_rows(&mut self, rows: &[Value], has_more: Option<bool>) {
        for r in rows {
            let id = r.get("rowId").and_then(Value::as_u64).unwrap_or(0);
            self.rows.entry(id).or_insert_with(|| Row::from_value(r));
        }
        if has_more == Some(false)
            && let Some(loaded) = self.loaded_first_row_id()
        {
            self.first_row_id = Some(loaded);
        }
    }

    /// Whole-key replacement of the `usage` region (sessionUsageStateSchema).
    fn apply_usage(&mut self, usage: &Value) {
        self.throughput.on_usage(usage, std::time::Instant::now());
        self.context_window = crate::conversation::context_usage::ContextWindow::from_usage(usage);
    }

    pub fn apply_deltas(&mut self, deltas: &[Value]) {
        for d in deltas {
            match d.get("op").and_then(Value::as_str).unwrap_or("") {
                "row.appended" | "row.upserted" => {
                    if let Some(r) = d.get("row") {
                        let id = r.get("rowId").and_then(Value::as_u64).unwrap_or(0);
                        let appended = d.get("op").and_then(Value::as_str) == Some("row.appended");
                        if crate::conversation::throughput::is_call_boundary_row(appended, r) {
                            self.throughput.on_call_boundary(std::time::Instant::now());
                        }
                        if appended && crate::conversation::throughput::is_model_output_row(r) {
                            self.throughput.on_model_row();
                        }
                        if appended {
                            self.first_row_id.get_or_insert(id);
                        }
                        self.note_compact_marker(r);
                        self.rows.insert(id, Row::from_value(r));
                    }
                }
                "row.removed" => {
                    let from_id = d
                        .get("fromRowId")
                        .or_else(|| d.get("rowId"))
                        .and_then(Value::as_u64);
                    if let Some(from_id) = from_id {
                        self.rows.retain(|&id, _| id < from_id);
                        // Shared reducer parity (zcode-protocol-v4/apply.ts):
                        // removing from the oldest row empties the branch.
                        if self.first_row_id.is_some_and(|first| from_id <= first) {
                            self.first_row_id = None;
                        }
                    }
                }
                "row.delta" => {
                    let path = d.get("path").and_then(Value::as_str).unwrap_or("text");
                    let append = d.get("append").and_then(Value::as_str).unwrap_or("");
                    if append.is_empty() {
                        continue;
                    }
                    if crate::conversation::throughput::is_model_output_path(path) {
                        self.throughput.on_stream(append, std::time::Instant::now());
                    }
                    if let Some(row) = d
                        .get("rowId")
                        .and_then(Value::as_u64)
                        .and_then(|id| self.rows.get_mut(&id))
                        && let Some(field) = row.stream_field_mut(path)
                    {
                        field.push_str(append);
                    }
                }
                "state.updated" => {
                    if let Some(patch) = d.get("patch") {
                        if let Some(usage) = patch.get("usage") {
                            self.apply_usage(usage);
                        }
                        if let Some(routing) = patch.get("inputRouting") {
                            self.input_routing =
                                crate::composer::delivery::InputRouting::from_value(routing);
                        }
                        if let Some(control) = patch.get("control") {
                            self.apply_control(control);
                        }
                        if let Some(config) = patch.get("config") {
                            self.apply_config(config);
                        }
                        if let Some(rev) = patch.get("revision").and_then(Value::as_u64) {
                            self.revision = rev;
                        }
                        if let Some(pis) =
                            patch.get("pendingInteractions").and_then(Value::as_array)
                        {
                            self.set_pending_interactions(pis);
                        }
                        if let Some(goal) = patch.get("goal") {
                            self.goal = crate::conversation::goal::GoalState::from_value(goal);
                        }
                        if let Some(availability) = patch.get("availability") {
                            self.queue_edit_allowed = availability
                                .pointer("/queueEdit/allowed")
                                .and_then(Value::as_bool)
                                == Some(true);
                            self.goal_availability =
                                crate::conversation::goal::GoalAvailability::from_value(
                                    availability,
                                );
                        }
                        if let Some(p) = patch.get("plan") {
                            self.plan = crate::conversation::turn_meta::PlanState::from_value(p);
                        }
                        if let Some(q) = patch.get("queue") {
                            self.queue = crate::conversation::queue::QueueState::from_value(q);
                        }
                        if let Some(wf) = patch.get("workflowRuns") {
                            self.workflow_runs =
                                crate::conversation::workflows::WorkflowRunsState::from_value(wf);
                        }
                        if let Some(s) = patch.get("subagents") {
                            self.subagents =
                                crate::conversation::subagents::SubagentsState::from_value(s);
                        }
                        if let Some(bw_val) = patch.get("backgroundWorks") {
                            self.background_works = bw_val
                                .as_array()
                                .map(|bw| {
                                    bw.iter()
                                        .filter_map(
                                            crate::conversation::subagents::BackgroundWork::from_value,
                                        )
                                        .collect()
                                })
                                .unwrap_or_default();
                        }
                    }
                }
                "workflowRun.updated" => {
                    self.workflow_runs.apply_updated(d);
                }
                "workflowRun.removed" => {
                    self.workflow_runs.apply_removed(d);
                }
                _ => {} // queue.*, ... intentionally ignored
            }
        }
    }
}

#[cfg(test)]
#[path = "model_history_tests.rs"]
mod history_tests;
#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
