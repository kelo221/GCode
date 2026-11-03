//! `providerSettings/read|mutate` and model connectivity tests against the
//! active workspace's agent (see PROVIDER_SETTINGS.md).

use super::{ProviderView, Request, TestStatus};
use crate::app::store::AppState;
use crate::backend::workspace::Pending;
use gpui::Context;
use serde_json::{Value, json};

const METHOD_NOT_FOUND: i64 = -32601;

impl AppState {
    fn provider_settings_target(&self) -> Option<(String, Value)> {
        let key = self.active_ws_key()?;
        let ws = self.ws(&key).filter(|ws| ws.started)?;
        let workspace = json!({ "workspacePath": ws.path, "workspaceKey": ws.key });
        Some((key, workspace))
    }

    /// Sends one page request; false when no started agent could take it.
    fn send_provider_settings(&mut self, request: Request, method: &str, params: Value) -> bool {
        let Some((key, _)) = self.provider_settings_target() else {
            self.provider_settings.error = Some(no_agent().into());
            return false;
        };
        let Some(ws) = self.ws_mut(&key) else {
            return false;
        };
        let id = ws.next_id();
        ws.pending.insert(id, Pending::ProviderSettings(request));
        self.send_request(&key, method, params, id);
        self.ws(&key).is_some_and(|ws| ws.pending.contains_key(&id))
    }

    pub(crate) fn read_provider_settings(&mut self, refresh: bool, cx: &mut Context<Self>) {
        if self.provider_settings.loading || self.provider_settings.busy {
            return;
        }
        self.provider_settings.error = None;
        self.provider_settings.loading = self.send_provider_settings(
            Request::Read,
            "providerSettings/read",
            json!({ "refresh": refresh }),
        );
        cx.notify();
    }

    /// One mutation at a time; later ones wait in `queued`.
    pub(crate) fn mutate_provider_settings(&mut self, operation: Value, cx: &mut Context<Self>) {
        self.send_mutation(operation, None, cx);
    }

    /// A mutation whose outcome lands in `provider_settings.added` under the
    /// returned tag instead of the page error (the add-model dialog shows it).
    pub(crate) fn mutate_provider_settings_tagged(
        &mut self,
        operation: Value,
        cx: &mut Context<Self>,
    ) -> u64 {
        let tag = self.next_provider_settings_tag();
        self.send_mutation(operation, Some(tag), cx);
        tag
    }

    fn next_provider_settings_tag(&mut self) -> u64 {
        self.provider_settings.next_tag += 1;
        self.provider_settings.next_tag
    }

    fn send_mutation(&mut self, operation: Value, tag: Option<u64>, cx: &mut Context<Self>) {
        if self.provider_settings.busy {
            self.provider_settings.queued.push_back((operation, tag));
            return;
        }
        self.provider_settings.error = None;
        self.provider_settings.busy = self.send_provider_settings(
            Request::Mutate { tag },
            "providerSettings/mutate",
            json!({ "operation": operation }),
        );
        if !self.provider_settings.busy
            && let Some(tag) = tag
        {
            let message = self.provider_settings.error.take();
            let message = message.unwrap_or_else(|| no_agent().into());
            self.provider_settings.added = Some((tag, Err(message)));
        }
        cx.notify();
    }

    /// Read-only recommended-config preview; not serialized with mutations.
    /// Returns the tag its outcome lands under in `provider_settings.resolved`.
    pub(crate) fn resolve_provider_model_config(
        &mut self,
        provider: &str,
        model: &str,
        builtin_only: bool,
        cx: &mut Context<Self>,
    ) -> u64 {
        let tag = self.next_provider_settings_tag();
        let mut params = json!({ "providerId": provider, "modelId": model });
        // 编辑已有模型：带空 personalConfig 时 inheritedConfig 只含内置推荐，
        // 否则会把该模型自己已保存的覆盖当成推荐基线。
        if builtin_only {
            params["personalConfig"] = json!({});
        }
        let previous = self.provider_settings.error.clone();
        if !self.send_provider_settings(
            Request::Resolve { tag },
            "providerSettings/resolveModelConfig",
            params,
        ) {
            // 预览失败只属于弹窗；不能把“无智能体”写成整页错误。
            let message = std::mem::replace(&mut self.provider_settings.error, previous);
            let message = message.unwrap_or_else(|| no_agent().into());
            self.provider_settings.resolved = Some((tag, Err(message)));
        }
        cx.notify();
        tag
    }

    pub(crate) fn test_provider_model(
        &mut self,
        provider: &str,
        model: &str,
        cx: &mut Context<Self>,
    ) {
        let Some((_, workspace)) = self.provider_settings_target() else {
            self.provider_settings.error = Some(no_agent().into());
            cx.notify();
            return;
        };
        let request = Request::Test {
            provider: provider.into(),
            model: model.into(),
        };
        let params = json!({
            "workspace": workspace,
            "selection": { "providerId": provider, "modelId": model },
        });
        if self.send_provider_settings(request, "provider/testModelConnectivity", params) {
            self.provider_settings
                .tests
                .insert((provider.into(), model.into()), TestStatus::Running);
        }
        cx.notify();
    }

    /// The mutating agent already refreshed its registry; the others re-read
    /// provider_config.json before their model catalogs are probed again.
    fn propagate_provider_change(&mut self, mutated: &str) {
        self.probe_model_catalog(mutated);
        let others: Vec<String> = self
            .workspaces
            .iter()
            .filter(|ws| ws.started && ws.key != mutated)
            .map(|ws| ws.key.clone())
            .collect();
        for key in others {
            let Some(ws) = self.ws_mut(&key) else {
                continue;
            };
            let id = ws.next_id();
            ws.pending
                .insert(id, Pending::ProviderSettings(Request::RefreshRegistry));
            let params = json!({ "reason": "gpui-provider-settings" });
            self.send_request(&key, "provider/refreshRegistry", params, id);
        }
    }

    pub(crate) fn handle_provider_settings_response(
        &mut self,
        ws_key: &str,
        id: u64,
        result: &Option<Value>,
        error: &Option<Value>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(ws) = self.ws_mut(ws_key) else {
            return false;
        };
        let Some(Pending::ProviderSettings(request)) = ws.pending.get(&id) else {
            return false;
        };
        let request = request.clone();
        ws.pending.remove(&id);
        let message = error.as_ref().map(|e| {
            e.get("message")
                .and_then(Value::as_str)
                .unwrap_or("rpc error")
                .to_owned()
        });
        let code = error
            .as_ref()
            .and_then(|e| e.get("code"))
            .and_then(Value::as_i64);
        match request {
            Request::RefreshRegistry => {
                // 刷新失败时该 agent 仍会在 ~1s 后轮询到文件变化；只在成功后重新探测目录。
                if message.is_none() {
                    self.probe_model_catalog(ws_key);
                }
            }
            Request::Test { provider, model } => {
                let status = match message {
                    Some(message) => TestStatus::Failed(message),
                    None => TestStatus::Passed,
                };
                self.provider_settings
                    .tests
                    .insert((provider, model), status);
            }
            Request::Resolve { tag } => {
                let outcome = match (message, result) {
                    (Some(message), _) => Err(message),
                    (None, Some(result)) => result
                        .get("inheritedConfig")
                        .filter(|config| config.is_object())
                        .cloned()
                        .ok_or_else(|| "invalid model config response".to_owned()),
                    (None, None) => Err("invalid model config response".into()),
                };
                self.provider_settings.resolved = Some((tag, outcome));
            }
            Request::Read | Request::Mutate { .. } => {
                let read = request == Request::Read;
                let tag = match request {
                    Request::Mutate { tag } => tag,
                    _ => None,
                };
                let state = &mut self.provider_settings;
                if read {
                    state.loading = false;
                } else {
                    state.busy = false;
                }
                if read && code == Some(METHOD_NOT_FOUND) {
                    state.unavailable = true;
                } else if let Some(message) = message {
                    // 带 tag 的变更由弹窗展示错误并保留输入，不写整页错误。
                    match tag {
                        Some(tag) => state.added = Some((tag, Err(message))),
                        None => state.error = Some(message),
                    }
                } else if let Some(result) = result {
                    self.apply_provider_settings_result(ws_key, read, result);
                    if let Some(tag) = tag {
                        let outcome = match self.provider_settings.error.take() {
                            Some(message) => Err(message),
                            None => Ok(()),
                        };
                        self.provider_settings.added = Some((tag, outcome));
                    }
                }
                if !read && let Some((next, tag)) = self.provider_settings.queued.pop_front() {
                    self.send_mutation(next, tag, cx);
                }
            }
        }
        cx.notify();
        true
    }

    fn apply_provider_settings_result(&mut self, ws_key: &str, read: bool, result: &Value) {
        let Some(view) = result.get("view").and_then(ProviderView::from_value) else {
            self.provider_settings.error = Some("invalid provider settings response".into());
            return;
        };
        let state = &mut self.provider_settings;
        state.unavailable = false;
        if let Some(created) = result.get("providerId").and_then(Value::as_str) {
            state.selected = Some(created.to_owned());
            state.adding = false;
        }
        if state
            .selected
            .as_deref()
            .is_some_and(|id| view.provider(id).is_none())
        {
            state.selected = None;
        }
        state.confirm_delete = None;
        state.view = Some(view);
        if !read {
            self.propagate_provider_change(ws_key);
        }
    }
}

fn no_agent() -> &'static str {
    crate::shared::i18n::label(
        "Open a workspace with a running agent to manage providers",
        "请先打开一个已连接智能体的工作区以管理提供商",
    )
}

#[cfg(test)]
#[path = "requests_tests.rs"]
mod tests;
