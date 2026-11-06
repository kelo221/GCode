use serde::Deserialize;
use std::collections::HashSet;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DirectoryAgent {
    pub child_session_id: String,
    pub agent_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub subagent_type: String,
    pub title: String,
    pub summary: Option<String>,
    pub status: String,
    pub started_at: Option<u64>,
    pub ended_at: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DirectoryResult {
    pub revision: u64,
    pub child_session_ids: Vec<String>,
    pub running: Vec<DirectoryAgent>,
    pub ended: EndedPage,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct EndedPage {
    pub total: u64,
    pub items: Vec<DirectoryAgent>,
    pub next_cursor: Option<String>,
}

#[derive(Default)]
pub(crate) struct SubagentDirectory {
    pub observed_revision: Option<u64>,
    pub revision: Option<u64>,
    pub items: Vec<DirectoryAgent>,
    /// Running children from the latest page. Only rendered for nested
    /// directories (a subagent's own children); the main session's running
    /// list comes from the live V4 projection instead.
    pub running: Vec<DirectoryAgent>,
    /// Nested directories only: when the last fetch was sent, and whether the
    /// parent subagent was still running then (so its end triggers one refetch).
    pub fetched_at: Option<std::time::Instant>,
    pub fetched_while_running: bool,
    pub next_cursor: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    pub staging: Vec<DirectoryAgent>,
    pub target_depth: usize,
    pub cursors: HashSet<String>,
}

impl DirectoryResult {
    pub(crate) fn parse(value: serde_json::Value) -> Result<Self, String> {
        let result: Self = serde_json::from_value(value)
            .map_err(|_| "Invalid subagent directory response".to_string())?;
        let nonblank = |s: &str| !s.trim().is_empty();
        let valid_agent = |a: &DirectoryAgent, ended: bool| {
            nonblank(&a.child_session_id)
                && nonblank(&a.subagent_type)
                && nonblank(&a.title)
                && a.agent_id.as_deref().is_none_or(nonblank)
                && a.tool_call_id.as_deref().is_none_or(nonblank)
                && if ended {
                    matches!(
                        a.status.as_str(),
                        "success" | "failed" | "cancelled" | "lost"
                    )
                } else {
                    matches!(a.status.as_str(), "running" | "waiting" | "blocked")
                }
        };
        if !result.child_session_ids.iter().all(|id| nonblank(id))
            || !result.running.iter().all(|a| valid_agent(a, false))
            || !result.ended.items.iter().all(|a| valid_agent(a, true))
            || result
                .ended
                .next_cursor
                .as_deref()
                .is_some_and(|s| !nonblank(s))
            || result.ended.items.len() > 100
            || result.ended.total < result.ended.items.len() as u64
        {
            return Err("Invalid subagent directory response".into());
        }
        Ok(result)
    }
}

pub(crate) fn merge_page(target: &mut Vec<DirectoryAgent>, items: Vec<DirectoryAgent>) {
    for item in items {
        if let Some(index) = target
            .iter()
            .position(|a| a.child_session_id == item.child_session_id)
        {
            target[index] = item;
        } else {
            target.push(item);
        }
    }
}
