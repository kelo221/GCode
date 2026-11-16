//! Subagent projection models, deserialization, and state joins for the V4 protocol.
//!
//! Spec sources:
//! - packages/shared/src/zcode-protocol-v4/snapshot.ts (subagentProjectionStateSchema, backgroundWorkSummarySchema)
//! - packages/ui/src/v4/conversationStatusPanelModel.ts (subagent × backgroundWorks join)

use crate::conversation::model::{ConversationState, Row};
use crate::conversation::turn_meta::PlanItem;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SubagentsState {
    pub revision: u64,
    pub child_session_ids: Vec<String>,
    pub running: Vec<RunningSubagent>,
    pub ended_total: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RunningSubagent {
    pub child_session_id: String,
    pub agent_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub subagent_type: String,
    pub title: String,
    pub summary: Option<String>,
    pub status: String,
    pub started_at: Option<u64>,
    /// `provider/model` the child actually runs on.
    #[serde(default)]
    pub model: Option<String>,
    /// The child's latest TodoWrite list (live only; empty after a cold reload
    /// until the child writes its todos again).
    #[serde(default)]
    pub todos: Vec<PlanItem>,
    /// `runningSubagentSummary.stopControl` (v3.18.2): present only for a live
    /// execution this CLI can stop with `stopSubagent`.
    #[serde(default)]
    pub stop_control: Option<SubagentStopControl>,
}

/// `{ taskId, executionId }` identifying one live subagent execution; the CLI
/// rejects a stop whose execution was replaced (SendMessage resume).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentStopControl {
    pub task_id: String,
    pub execution_id: String,
}

impl SubagentStopControl {
    /// Both ids must be non-empty (`z.string().min(1)`, strict object).
    pub fn from_value(v: &Value) -> Option<Self> {
        let field = |k: &str| {
            v.get(k)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        Some(Self {
            task_id: field("taskId")?,
            execution_id: field("executionId")?,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BackgroundWork {
    pub work_id: String,
    pub kind: String,
    pub title: String,
    pub status: String,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub cancellable: Option<bool>,
    pub blocked: Option<bool>,
    pub anchor_row_id: Option<u64>,
    pub child_session_id: Option<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub struct JoinedRunningSubagent {
    pub child_session_id: String,
    pub agent_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub subagent_type: String,
    pub title: String,
    pub summary: Option<String>,
    pub status: String,
    pub started_at: Option<u64>,
    pub model: Option<String>,
    pub todos: Vec<PlanItem>,
    pub work_id: Option<String>,
    pub cancellable: bool,
    pub stop_control: Option<SubagentStopControl>,
}

impl SubagentsState {
    pub fn from_value(v: &Value) -> Option<Self> {
        if v.is_null() {
            return None;
        }
        let revision = v.get("revision").and_then(Value::as_u64).unwrap_or(0);
        let child_session_ids = v
            .get("childSessionIds")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let running = v
            .get("running")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(RunningSubagent::from_value).collect())
            .unwrap_or_default();
        let ended_total = v.get("endedTotal").and_then(Value::as_u64).unwrap_or(0);
        Some(Self {
            revision,
            child_session_ids,
            running,
            ended_total,
        })
    }
}

impl RunningSubagent {
    pub fn from_value(v: &Value) -> Option<Self> {
        let child_session_id = v.get("childSessionId").and_then(Value::as_str)?.to_string();
        let agent_id = v.get("agentId").and_then(Value::as_str).map(str::to_string);
        let tool_call_id = v
            .get("toolCallId")
            .and_then(Value::as_str)
            .map(str::to_string);
        let subagent_type = v
            .get("subagentType")
            .and_then(Value::as_str)
            .unwrap_or("subagent")
            .to_string();
        let title = v
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let summary = v.get("summary").and_then(Value::as_str).map(str::to_string);
        let status = v
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("running")
            .to_string();
        let started_at = v.get("startedAt").and_then(Value::as_u64);
        let model = v
            .get("model")
            .and_then(Value::as_str)
            .filter(|m| !m.trim().is_empty())
            .map(str::to_string);
        let todos = v
            .get("todos")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(PlanItem::from_value).collect())
            .unwrap_or_default();
        let stop_control = v
            .get("stopControl")
            .and_then(SubagentStopControl::from_value);

        Some(Self {
            child_session_id,
            agent_id,
            tool_call_id,
            subagent_type,
            title,
            summary,
            status,
            started_at,
            model,
            todos,
            stop_control,
        })
    }
}

impl BackgroundWork {
    pub fn from_value(v: &Value) -> Option<Self> {
        let work_id = v.get("workId").and_then(Value::as_str)?.to_string();
        let kind = v
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let title = v
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let status = v
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let started_at = v.get("startedAt").and_then(Value::as_u64).unwrap_or(0);
        let ended_at = v.get("endedAt").and_then(Value::as_u64);
        let cancellable = v.get("cancellable").and_then(Value::as_bool);
        let blocked = v.get("blocked").and_then(Value::as_bool);
        let anchor_row_id = v.get("anchorRowId").and_then(Value::as_u64);
        let child_session_id = v
            .get("childSessionId")
            .and_then(Value::as_str)
            .map(str::to_string);

        Some(Self {
            work_id,
            kind,
            title,
            status,
            started_at,
            ended_at,
            cancellable,
            blocked,
            anchor_row_id,
            child_session_id,
        })
    }
}

/// Join running subagents with backgroundWorks to attach control workId and cancellable status.
#[allow(dead_code)]
pub fn join_running_subagents(
    subagents: Option<&SubagentsState>,
    background_works: &[BackgroundWork],
) -> Vec<JoinedRunningSubagent> {
    let mut control_by_child_sid: HashMap<&str, Option<&BackgroundWork>> = HashMap::new();
    for work in background_works {
        if work.status != "running" || work.kind != "subagent" {
            continue;
        }
        if let Some(ref child_sid) = work.child_session_id {
            control_by_child_sid
                .entry(child_sid.as_str())
                .and_modify(|existing| *existing = None)
                .or_insert(Some(work));
        }
    }

    let mut result = Vec::new();
    let mut projected_child_sids = HashSet::new();

    if let Some(subagents) = subagents {
        for sub in &subagents.running {
            projected_child_sids.insert(sub.child_session_id.as_str());
            let (work_id, cancellable) =
                match control_by_child_sid.get(sub.child_session_id.as_str()) {
                    Some(Some(w)) => (Some(w.work_id.clone()), w.cancellable != Some(false)),
                    _ => (None, false),
                };
            result.push(JoinedRunningSubagent {
                child_session_id: sub.child_session_id.clone(),
                agent_id: sub.agent_id.clone(),
                tool_call_id: sub.tool_call_id.clone(),
                subagent_type: sub.subagent_type.clone(),
                title: sub.title.clone(),
                summary: sub.summary.clone(),
                status: sub.status.clone(),
                started_at: sub.started_at,
                model: sub.model.clone(),
                todos: sub.todos.clone(),
                work_id,
                cancellable,
                stop_control: sub.stop_control.clone(),
            });
        }
    }

    for (&child_sid, maybe_work) in &control_by_child_sid {
        if let Some(work) = maybe_work
            && !projected_child_sids.contains(child_sid)
        {
            result.push(JoinedRunningSubagent {
                child_session_id: child_sid.to_string(),
                agent_id: Some(work.work_id.clone()),
                tool_call_id: None,
                subagent_type: "subagent".to_string(),
                title: work.title.clone(),
                summary: None,
                status: if work.blocked == Some(true) {
                    "blocked".to_string()
                } else {
                    "running".to_string()
                },
                started_at: Some(work.started_at),
                model: None,
                todos: Vec::new(),
                work_id: Some(work.work_id.clone()),
                cancellable: work.cancellable != Some(false),
                stop_control: None,
            });
        }
    }

    result
}

impl ConversationState {
    pub fn find_subagent_for_tool_call(&self, tool_call_id: &str) -> Option<&Row> {
        self.rows.values().find(|r| match r {
            Row::Subagent {
                parent_tool_call_id: Some(ptid),
                ..
            } => ptid == tool_call_id,
            _ => false,
        })
    }

    pub fn has_tool_call_with_id(&self, tool_call_id: &str) -> bool {
        self.rows.values().any(|r| match r {
            Row::ToolCall {
                tool_call_id: Some(tcid),
                ..
            } => tcid == tool_call_id,
            _ => false,
        })
    }

    pub fn subagent_work_control(
        &self,
        work_id: Option<&str>,
        child_session_id: Option<&str>,
    ) -> (Option<String>, bool) {
        for w in &self.background_works {
            let matches_id = work_id.is_some_and(|id| w.work_id == id);
            let matches_child =
                child_session_id.is_some_and(|cid| w.child_session_id.as_deref() == Some(cid));
            if (matches_id || matches_child) && w.status == "running" && w.kind == "subagent" {
                return (Some(w.work_id.clone()), w.cancellable != Some(false));
            }
        }
        if let Some(id) = work_id {
            (Some(id.to_string()), true)
        } else {
            (None, false)
        }
    }
}

#[cfg(test)]
#[path = "subagents_tests.rs"]
mod tests;
