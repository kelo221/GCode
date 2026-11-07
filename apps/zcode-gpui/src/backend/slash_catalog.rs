use crate::app::store::AppState;
use crate::backend::workspace::Pending;
use crate::composer::slash_catalog::{SLASH_CATALOG_ERROR, parse_commands};
use gpui::Context;
use serde_json::{Value, json};

impl AppState {
    pub(crate) fn ensure_slash_catalog(&mut self, refresh: bool, cx: &mut Context<Self>) {
        if self.is_read_only_view() {
            return;
        }
        let Some(key) = self.active_ws_key() else {
            return;
        };
        let session = self.active.clone();
        // 修复原因：`session/read` 要求会话已在 agent 上加载（requireSession）；空闲卸载后
        // 新 agent 尚未重新订阅当前会话时请求必然失败，界面停在 "Refresh to retry"。
        // 未订阅时先不请求，订阅完成后的重绘会再次调用这里。
        if let Some(sid) = &session
            && !self.conversations.get(sid).is_some_and(|c| c.subscribed)
        {
            return;
        }
        let Some(ws) = self.ws_mut(&key) else { return };
        if !ws.started || ws.inbound.is_none() {
            return;
        }
        let query = ws.slash_catalogs.entry(session.clone()).or_default();
        if query.loading || (!refresh && query.attempted) {
            return;
        }
        query.start();
        let (method, params) = match &session {
            Some(sid) => ("session/read", json!({"sessionId":sid,"messageLimit":1})),
            None => (
                "workspace/readPresentation",
                json!({"workspace":{"workspacePath":ws.path.to_string_lossy(),"workspaceKey":ws.key}}),
            ),
        };
        let id = ws.next_id();
        ws.pending
            .insert(id, Pending::SlashCatalog(session.clone()));
        if !ws.send_pending_line(
            id,
            json!({"id":id,"method":method,"params":params}).to_string(),
        ) {
            ws.slash_catalogs
                .entry(session)
                .or_default()
                .fail(SLASH_CATALOG_ERROR);
        }
        cx.notify();
    }

    pub(crate) fn settle_slash_catalog(
        &mut self,
        workspace: &str,
        session: Option<String>,
        value: Option<Value>,
    ) {
        let Some(ws) = self.ws_mut(workspace) else {
            return;
        };
        let parsed = value.ok_or(SLASH_CATALOG_ERROR).and_then(|value| {
            let owner = match &session {
                Some(sid)
                    if value.pointer("/session/sessionId").and_then(Value::as_str) == Some(sid) =>
                {
                    value.pointer("/session/workspace")
                }
                Some(_) => None,
                None if matches!(
                    value.get("mode").and_then(Value::as_str),
                    Some("plan" | "build" | "edit" | "yolo" | "auto")
                ) =>
                {
                    value.get("workspace")
                }
                None => None,
            }
            .ok_or(SLASH_CATALOG_ERROR)?;
            if owner.get("workspaceKey").and_then(Value::as_str) != Some(ws.key.as_str())
                || owner.get("workspacePath").and_then(Value::as_str)
                    != Some(ws.path.to_string_lossy().as_ref())
            {
                return Err(SLASH_CATALOG_ERROR);
            }
            parse_commands(value.get("slashCommands").ok_or(SLASH_CATALOG_ERROR)?)
                .map_err(|_| SLASH_CATALOG_ERROR)
        });
        let query = ws.slash_catalogs.entry(session).or_default();
        match parsed {
            Ok(commands) => query.finish(commands),
            Err(_) => query.fail(SLASH_CATALOG_ERROR),
        }
    }
}

#[cfg(test)]
#[path = "slash_catalog_tests.rs"]
mod tests;
