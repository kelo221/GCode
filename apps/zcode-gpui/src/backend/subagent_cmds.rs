use crate::app::store::AppState;
use crate::backend::workspace::Pending;
use crate::conversation::subagent_directory::{DirectoryResult, merge_page};
use gpui::Context;
use serde_json::{Value, json};
use std::time::Duration;

const NESTED_POLL: Duration = Duration::from_secs(3);

impl AppState {
    pub(crate) fn ensure_subagent_directory(
        &mut self,
        refresh: bool,
        more: bool,
        cx: &mut Context<Self>,
    ) {
        let Some((key, sid)) = self.active_ws_key().zip(self.active.clone()) else {
            return;
        };
        if self.is_read_only_view() {
            return;
        }
        let revision = self
            .conversations
            .get(&sid)
            .and_then(|c| c.subagents.as_ref())
            .map(|s| s.revision)
            .unwrap_or(0);
        let Some(ws) = self.ws_mut(&key) else {
            return;
        };
        if !ws.started || ws.inbound.is_none() {
            return;
        }
        let cache = ws.subagent_directories.entry(sid.clone()).or_default();
        if cache.loading {
            return;
        }
        let append = more && cache.observed_revision == Some(revision);
        let cursor = if append {
            let Some(cursor) = cache.next_cursor.clone() else {
                return;
            };
            Some(cursor)
        } else {
            if !refresh && cache.observed_revision == Some(revision) {
                return;
            }
            cache.target_depth = cache.items.len();
            cache.staging.clear();
            cache.cursors.clear();
            cache.observed_revision = Some(revision);
            None
        };
        self.request_subagent_page(&key, &sid, cursor, !append, cx);
    }

    /// Fetch the nested Explore children of the active session's subagent
    /// `child_sid` (main view only). Their spawn events live on the subagent's
    /// own event stream, so the main projection never carries them; poll
    /// `session/subagents` for the subagent every few seconds while it runs and
    /// once more after it ends.
    pub(crate) fn ensure_nested_subagents(
        &mut self,
        child_sid: &str,
        parent_running: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = self.active_ws_key() else {
            return;
        };
        if self.is_read_only_view() || child_sid.trim().is_empty() {
            return;
        }
        let Some(ws) = self.ws_mut(&key) else {
            return;
        };
        if !ws.started || ws.inbound.is_none() {
            return;
        }
        let cache = ws.subagent_directories.entry(child_sid.into()).or_default();
        let due = match cache.fetched_at {
            None => true,
            Some(at) if parent_running => at.elapsed() >= NESTED_POLL,
            Some(_) => cache.fetched_while_running,
        };
        if cache.loading || !due {
            return;
        }
        cache.fetched_at = Some(std::time::Instant::now());
        cache.fetched_while_running = parent_running;
        cache.target_depth = 0;
        cache.staging.clear();
        cache.cursors.clear();
        self.request_subagent_page(&key, child_sid, None, true, cx);
        if parent_running {
            // 子代理阻塞等待嵌套 Explore 时主会话没有事件，面板不会重绘；定时唤醒以继续轮询。
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(NESTED_POLL).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
    }

    fn request_subagent_page(
        &mut self,
        key: &str,
        sid: &str,
        cursor: Option<String>,
        replacing: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(ws) = self.ws_mut(key) else {
            return;
        };
        let cache = ws.subagent_directories.entry(sid.into()).or_default();
        if let Some(cursor) = &cursor
            && !cache.cursors.insert(cursor.clone())
        {
            cache.loading = false;
            cache.error = Some("Subagent directory returned a repeated cursor".into());
            return;
        }
        cache.loading = true;
        cache.error = None;
        let mut params = json!({"sessionId":sid,"endedLimit":20});
        if let Some(cursor) = cursor {
            params["endedCursor"] = json!(cursor);
        }
        let id = ws.next_id();
        ws.pending.insert(
            id,
            Pending::SubagentDirectory {
                session: sid.into(),
                replacing,
            },
        );
        if !ws.send_pending_line(
            id,
            json!({"id":id,"method":"session/subagents","params":params}).to_string(),
        ) {
            let cache = ws.subagent_directories.get_mut(sid).unwrap();
            cache.loading = false;
            cache.error = Some("Subagent directory request could not be sent".into());
        }
        cx.notify();
    }

    pub(crate) fn settle_subagent_directory(
        &mut self,
        key: &str,
        sid: &str,
        replacing: bool,
        result: Result<Value, String>,
        cx: &mut Context<Self>,
    ) {
        let result = result.and_then(DirectoryResult::parse);
        let Some(cache) = self
            .ws_mut(key)
            .and_then(|ws| ws.subagent_directories.get_mut(sid))
        else {
            return;
        };
        cache.loading = false;
        match result {
            Ok(result) => {
                cache.revision = Some(result.revision);
                cache.running = result.running;
                if replacing {
                    merge_page(&mut cache.staging, result.ended.items);
                    if cache.staging.len() < cache.target_depth
                        && result.ended.next_cursor.is_some()
                    {
                        self.request_subagent_page(key, sid, result.ended.next_cursor, true, cx);
                        return;
                    }
                    cache.items = std::mem::take(&mut cache.staging);
                } else {
                    merge_page(&mut cache.items, result.ended.items);
                }
                cache.next_cursor = result.ended.next_cursor;
                cache.error = None;
            }
            Err(error) => {
                cache.error = Some(crate::shared::redact::scrub(&error));
                cache.staging.clear();
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
#[path = "subagent_cmds_tests.rs"]
mod tests;
