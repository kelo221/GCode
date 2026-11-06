//! Topic resynchronization, backpressure flow flags, and history paging.

use crate::app::store::AppState;
use crate::backend::workspace::Pending;
use gpui::{AppContext, Context};
use serde_json::{Value, json};

impl AppState {
    /// Force a fresh snapshot for a subscribed topic after a fault (CRC
    /// failure, seq gap, assembly timeout). Same subscriptionId, so the
    /// server keeps the route and re-delivers from `deliveryKind:"recovery"`.
    pub(crate) fn resync_topic(&mut self, ws_key: &str, topic: &str) {
        let Some(ws) = self.ws_mut(ws_key) else {
            return;
        };
        if !ws.started {
            return;
        }
        let Some(sub) = ws.subscriptions.get(topic).map(|s| s.id.clone()) else {
            return;
        };
        let id = ws.next_id();
        ws.pending.insert(id, Pending::Resync(topic.to_string()));
        let conn_id = ws.connection_id.clone();
        let base = Value::Null;
        self.push_log(format!("resyncing {topic}"));
        self.send_request(
            ws_key,
            "v4/conversation/resync",
            json!({
                "topic": topic,
                "subscriptionId": sub,
                "connectionId": conn_id,
                "base": base,
                "forceSnapshot": true,
            }),
            id,
        );
    }

    /// Background freshness probe for the open conversation (cursorless
    /// `v4/conversation/rowsRange`). The response is never applied: it is only
    /// compared for movement, and movement triggers the standard resync whose
    /// snapshot repaints the view. This is what keeps sessions hosted by
    /// ANOTHER process (deltas never reach our backend) up to date.
    pub fn poll_conversation_tail(&mut self, _cx: &mut Context<Self>) {
        let (Some(sid), Some(ws_key)) = (self.active.clone(), self.active_ws_key()) else {
            return;
        };
        let subscribed = match self.conversations.get(&sid) {
            Some(c) => c.subscribed,
            None => return,
        };
        if !subscribed {
            return;
        }
        let Some(ws) = self.ws_mut(&ws_key) else {
            return;
        };
        if !ws.started {
            return;
        }
        // One probe in flight at a time.
        if ws
            .pending
            .values()
            .any(|p| matches!(p, Pending::PollRows(_)))
        {
            return;
        }
        let id = ws.next_id();
        ws.pending.insert(id, Pending::PollRows(sid.clone()));
        ws.send_pending_line(
            id,
            json!({
                "id": id,
                "method": "v4/conversation/rowsRange",
                "params": {
                    "sessionId": sid,
                    "limit": 30,
                }
            })
            .to_string(),
        );
    }

    /// Page in history older than the oldest loaded row
    /// (`v4/conversation/rowsRange`, cursor = that row's id).
    pub fn fetch_earlier_rows(&mut self, cx: &mut Context<Self>) {
        // 修复原因：按钮依据 active_conversation()（打开子代理时是 child）显示，这里却取
        // self.active（父会话）——父会话没有更早历史时直接 return，按钮点了没反应。
        // 与按钮同源：正在看的 child 优先，workspace 取 child_owner 登记的那个。
        let sid = match self.viewing_child.clone() {
            Some(child) => Some(child),
            None => self.active.clone(),
        };
        let ws_key = sid
            .as_ref()
            .and_then(|sid| self.child_owner.get(sid))
            .map(|(ws, _)| ws.clone())
            .or_else(|| self.active_ws_key());
        let (Some(sid), Some(ws_key)) = (sid, ws_key) else {
            return;
        };
        let (before, subscribed) = match self.conversations.get(&sid) {
            Some(c) if c.has_more_history() => (c.loaded_first_row_id(), c.subscribed),
            _ => return,
        };
        let Some(before) = before else { return };
        if !subscribed {
            self.subscribe_conversation(&ws_key, &sid);
            cx.notify();
            return;
        }
        let Some(ws) = self.ws_mut(&ws_key) else {
            return;
        };
        let id = ws.next_id();
        ws.pending.insert(id, Pending::FetchRows(sid.clone()));
        ws.send_pending_line(
            id,
            json!({
                "id": id,
                "method": "v4/conversation/rowsRange",
                "params": {
                    "sessionId": sid,
                    "beforeRowId": before,
                    "limit": 200,
                }
            })
            .to_string(),
        );
        cx.notify();
    }

    /// Backpressure signal for the CLI's frame pump
    /// (`v4/connection/flow`, fire-and-forget with an empty result).
    pub(crate) fn send_flow_flag(&mut self, ws_key: &str, saturated: bool) {
        let Some(ws) = self.ws_mut(ws_key) else {
            return;
        };
        if !ws.started {
            return;
        }
        let conn_id = ws.connection_id.clone();
        // v4/connection/flow is a request with an empty result; its response
        // carries no pending entry and is ignored by the correlation layer.
        let id = ws.next_id();
        let state = if saturated { "saturated" } else { "drained" };
        ws.send_pending_line(
            id,
            json!({
                "id": id,
                "method": "v4/connection/flow",
                "params": {
                    "connectionId": conn_id,
                    "state": state,
                }
            })
            .to_string(),
        );
    }
}

/// Periodically probe the open conversation for movement (default 3s,
/// `ZCODE_GPUI_POLL_SECS` to tune). Sessions hosted by ANOTHER process —
/// e.g. the desktop app driving the same workspace — never push deltas to
/// our backend; the probe makes its cold projection refresh from the shared
/// store and repaints the view through the standard resync snapshot.
pub(crate) fn start_tail_poll(cx: &mut Context<AppState>) {
    let secs = std::env::var("ZCODE_GPUI_POLL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(3)
        .max(1);
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_spawn(async move {
                std::thread::sleep(std::time::Duration::from_secs(secs));
            })
            .await;
            let alive = this
                .update(cx, |state, cx| {
                    state.poll_conversation_tail(cx);
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !alive {
                break;
            }
        }
    })
    .detach();
}
