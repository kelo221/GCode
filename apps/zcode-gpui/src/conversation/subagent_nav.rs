//! Subagent child session navigation and read-only gating.
//!
//! Spec: Child conversation is an ordinary topic `conversation/<childSessionId>`.
//! The backend rejects input commands to child sessions (guard.subagentReadOnly).
//! The child view is strictly read-only: composer and turn actions are disabled.

use crate::app::store::AppState;
use gpui::Context;
use serde_json::json;
use std::collections::{HashMap, HashSet};

/// Collect all child session IDs from known child owners and loaded subagent states.
pub fn collect_child_session_ids<'a>(
    child_owner: &'a HashMap<String, (String, String)>,
    subagent_states: impl IntoIterator<Item = &'a crate::conversation::subagents::SubagentsState>,
) -> HashSet<&'a str> {
    let mut ids = HashSet::new();
    for key in child_owner.keys() {
        ids.insert(key.as_str());
    }
    for sub in subagent_states {
        for id in &sub.child_session_ids {
            ids.insert(id.as_str());
        }
    }
    ids
}

/// Pure predicate checking if viewing child is set.
pub fn is_read_only(viewing_child: Option<&str>) -> bool {
    viewing_child.is_some()
}

/// Look up parent session id for the current child view.
pub fn parent_session_id<'a>(
    child_owner: &'a HashMap<String, (String, String)>,
    viewing_child: Option<&str>,
) -> Option<&'a str> {
    let child_sid = viewing_child?;
    child_owner.get(child_sid).map(|(_, p)| p.as_str())
}

/// Look up workspace for a child session topic.
pub fn child_topic_workspace(
    child_owner: &HashMap<String, (String, String)>,
    topic: &str,
) -> Option<String> {
    let child_sid = topic.strip_prefix("conversation/")?;
    child_owner.get(child_sid).map(|(ws, _)| ws.clone())
}

/// Determine active conversation id between child and parent.
pub fn active_conversation_sid<'a>(
    viewing_child: Option<&'a str>,
    active_parent: Option<&'a str>,
) -> Option<&'a str> {
    viewing_child.or(active_parent)
}

impl AppState {
    /// Whether the current view is a read-only subagent child view.
    pub fn is_read_only_view(&self) -> bool {
        is_read_only(self.viewing_child.as_deref())
    }

    /// Collect all known child subagent session IDs across child_owner and
    /// loaded conversations' subagents projections.
    pub fn all_child_session_ids(&self) -> HashSet<&str> {
        collect_child_session_ids(
            &self.child_owner,
            self.conversations
                .values()
                .filter_map(|c| c.subagents.as_ref()),
        )
    }

    /// Return the parent session id of the currently viewed subagent child, if any.
    pub fn parent_of_viewing_child(&self) -> Option<String> {
        parent_session_id(&self.child_owner, self.viewing_child.as_deref()).map(str::to_string)
    }

    /// Open a child subagent conversation in read-only mode.
    #[cfg(test)]
    pub fn open_subagent(&mut self, child_sid: &str, cx: &mut Context<Self>) {
        let Some(parent_sid) = self.active.clone() else {
            return;
        };
        let Some(ws_key) = self.active_ws_key() else {
            return;
        };

        self.open_subagent_for(&ws_key, &parent_sid, child_sid, cx);
    }

    /// Owner to record when an Open button is pressed in the current view: the
    /// viewed subagent when inside one (its nested Explore children), else the
    /// active session.
    pub fn subagent_open_owner(&self) -> Option<(String, String)> {
        self.active_ws_key()
            .zip(self.viewing_child.clone().or_else(|| self.active.clone()))
    }

    /// Title of a subagent of the active session, from its live running list or
    /// the ended directory.
    pub fn known_subagent_title(&self, child_sid: &str) -> Option<String> {
        let active = self.active.as_deref()?;
        let running = self
            .conversations
            .get(active)
            .and_then(|c| c.subagents.as_ref())
            .and_then(|s| s.running.iter().find(|r| r.child_session_id == child_sid))
            .map(|r| r.title.clone());
        running
            .or_else(|| {
                let ws = self.ws(&self.active_ws_key()?)?;
                let cache = ws.subagent_directories.get(active)?;
                cache
                    .items
                    .iter()
                    .chain(&cache.running)
                    .find(|a| a.child_session_id == child_sid)
                    .map(|a| a.title.clone())
            })
            .filter(|t| !t.trim().is_empty())
    }

    /// Whether `child_sid` is a known subagent of `parent_sid` in `ws_key`, from
    /// the owner map, the subagent directory cache, or a loaded conversation.
    fn is_known_child_of(&self, ws_key: &str, parent_sid: &str, child_sid: &str) -> bool {
        self.child_owner
            .get(child_sid)
            .is_some_and(|(ws, parent)| ws == ws_key && parent == parent_sid)
            || self
                .ws(ws_key)
                .and_then(|ws| ws.subagent_directories.get(parent_sid))
                .is_some_and(|cache| {
                    cache
                        .items
                        .iter()
                        .chain(&cache.running)
                        .any(|agent| agent.child_session_id == child_sid)
                })
            || self.conversations.get(parent_sid).is_some_and(|conv| {
                conv.subagents.as_ref().is_some_and(|sub| {
                    sub.child_session_ids.iter().any(|sid| sid == child_sid)
                        || sub.running.iter().any(|sub| sub.child_session_id == child_sid)
                }) || conv.background_works.iter().any(|work| {
                    work.kind == "subagent" && work.child_session_id.as_deref() == Some(child_sid)
                }) || conv.rows.values().any(|row| matches!(row,
                    crate::conversation::model::Row::Subagent { child_session_id: Some(sid), .. }
                    if sid == child_sid
                ))
            })
    }

    pub(crate) fn open_subagent_for(
        &mut self,
        ws_key: &str,
        parent_sid: &str,
        child_sid: &str,
        cx: &mut Context<Self>,
    ) {
        // 旧面板按钮不能把别的父会话的 child 重新归属；先核验捕获的 owner 与投影。
        let Some(active) = self.active.clone() else {
            return;
        };
        if self.active_ws_key().as_deref() != Some(ws_key)
            || child_sid.trim().is_empty()
            || child_sid == parent_sid
            || child_sid == active
            || self
                .child_owner
                .get(child_sid)
                .is_some_and(|(ws, parent)| ws != ws_key || parent != parent_sid)
        {
            return;
        }
        // 修复原因：subagent 派发的嵌套 Explore（孙会话）的父是该 subagent 而非主会话，
        // 旧校验要求 parent == active，导致孙会话的 Open 被静默丢弃。父可以是主会话，
        // 或主会话已知的 subagent（嵌套深度仍为 2，与 CLI 只允许一层嵌套一致）。
        if parent_sid != active && !self.is_known_child_of(ws_key, &active, parent_sid) {
            return;
        }
        if !self.is_known_child_of(ws_key, parent_sid, child_sid) {
            return;
        }
        if self
            .viewing_child
            .as_deref()
            .is_some_and(|sid| sid != child_sid)
        {
            self.clear_subagent_view();
        }
        self.child_owner.insert(
            child_sid.to_string(),
            (ws_key.to_string(), parent_sid.to_string()),
        );
        self.viewing_child = Some(child_sid.to_string());

        // Subscribe to the child conversation if not already subscribed.
        let is_subbed = self
            .conversations
            .get(child_sid)
            .is_some_and(|c| c.subscribed);
        if !is_subbed {
            self.subscribe_conversation(ws_key, child_sid);
        }

        cx.notify();
    }

    /// Close the subagent view and go up one level: to the parent subagent for
    /// a nested Explore, else back to the main conversation.
    pub fn close_subagent(&mut self, cx: &mut Context<Self>) {
        let parent = self
            .viewing_child
            .as_ref()
            .and_then(|child| self.child_owner.get(child))
            .cloned();
        self.clear_subagent_view();
        // 嵌套深度最多 2：非主会话的父必然是主会话的 subagent（从主视图直接打开孙会话时
        // child_owner 里还没有它），所以回到它即可。
        if let Some((ws_key, parent_sid)) = parent
            && let Some(active) = self.active.clone()
            && active != parent_sid
        {
            self.open_subagent_for(&ws_key, &active, &parent_sid, cx);
        }
        cx.notify();
    }

    /// Leave the child view (also on session or workspace switch) and release
    /// the child's subscription and mirror.
    pub fn clear_subagent_view(&mut self) {
        if let Some(child_sid) = self.viewing_child.take() {
            self.release_child(&child_sid);
        }
    }

    /// Unsubscribe a child conversation and drop every local trace of the
    /// subscription. Leaving `subscribed`/the subscription id behind would make
    /// a later `open_subagent` skip the resubscribe and show a frozen mirror;
    /// dropping the mirror keeps closed children from holding memory.
    fn release_child(&mut self, child_sid: &str) {
        let topic = format!("conversation/{child_sid}");
        let Some(ws_key) = self.child_owner.get(child_sid).map(|(ws, _)| ws.clone()) else {
            return;
        };
        let sub = self.ws_mut(&ws_key).and_then(|ws| {
            let sub_id = ws.subscriptions.remove(&topic).map(|s| s.id)?;
            Some((sub_id, ws.connection_id.clone(), ws.next_id()))
        });
        if let Some((sub_id, conn_id, id)) = sub {
            self.send_request(
                &ws_key,
                "v4/conversation/unsubscribe",
                json!({ "topic": topic, "subscriptionId": sub_id, "connectionId": conn_id }),
                id,
            );
        }
        self.route_cursors.remove(&topic);
        self.assembler.forget_topics(&[topic]);
        self.conversations.remove(child_sid);
    }
}

#[cfg(test)]
#[path = "subagent_nav_tests.rs"]
mod tests;
