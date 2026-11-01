//! `secrets/sync` (GPUI only, SECRETS.md): each workspace agent keeps its
//! in-scope secrets in memory, replaced whole on every sync. Sent when the
//! agent's storage becomes ready, after the startup read of the OS store, and
//! after every vault edit.

use crate::app::store::AppState;
use crate::backend::workspace::Pending;
use gpui::{AppContext, Context};
use serde_json::{Value, json};

const METHOD_NOT_FOUND: i64 = -32601;

impl AppState {
    /// Startup: read every indexed value off the UI thread (the OS store can
    /// block, e.g. a Keychain prompt), then push to the agents already up.
    pub(crate) fn load_secrets(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.secrets.store() else {
            return;
        };
        let names: Vec<String> = self
            .secrets
            .entries
            .iter()
            .map(|m| m.name.clone())
            .collect();
        cx.spawn(async move |this, cx| {
            let (values, error) = cx
                .background_spawn(
                    async move { crate::secrets::read_values(store.as_ref(), &names) },
                )
                .await;
            let _ = this.update(cx, |state, cx| {
                state.secrets.finish_load(values, error);
                state.push_secrets_to_all();
                cx.notify();
            });
        })
        .detach();
    }

    /// Storage-ready hook: a fresh agent starts with an empty vault, so only a
    /// non-empty table needs sending.
    pub(crate) fn sync_secrets(&mut self, ws_key: &str) {
        if self.secrets.loaded && !self.secrets.secrets_for(ws_key).is_empty() {
            self.send_secrets(ws_key);
        }
    }

    fn push_secrets_to_all(&mut self) {
        let keys: Vec<String> = self
            .workspaces
            .iter()
            .filter(|ws| ws.started)
            .map(|ws| ws.key.clone())
            .collect();
        for key in keys {
            self.send_secrets(&key);
        }
    }

    fn send_secrets(&mut self, ws_key: &str) {
        if !self.secrets.loaded {
            return;
        }
        let secrets: Vec<Value> = self
            .secrets
            .secrets_for(ws_key)
            .into_iter()
            .map(|(name, value)| json!({ "name": name, "value": value }))
            .collect();
        let Some(ws) = self.ws_mut(ws_key).filter(|ws| ws.started) else {
            return;
        };
        let workspace = json!({ "workspacePath": ws.path, "workspaceKey": ws.key });
        let id = ws.next_id();
        ws.pending.insert(id, Pending::Secrets);
        let params = json!({ "workspace": workspace, "secrets": secrets });
        self.send_request(ws_key, "secrets/sync", params, id);
    }

    /// Add or replace a secret, then re-sync every running agent.
    pub(crate) fn save_secret(
        &mut self,
        name: &str,
        value: &str,
        workspaces: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let result = self.secrets.upsert(name, value, workspaces);
        self.after_secret_edit(&result, cx);
        result
    }

    pub(crate) fn delete_secret(&mut self, name: &str, cx: &mut Context<Self>) {
        let result = self.secrets.remove(name);
        self.after_secret_edit(&result, cx);
    }

    pub(crate) fn toggle_secret_workspace(
        &mut self,
        name: &str,
        ws_key: &str,
        cx: &mut Context<Self>,
    ) {
        let result = self.secrets.toggle_workspace(name, ws_key);
        self.after_secret_edit(&result, cx);
    }

    fn after_secret_edit(&mut self, result: &Result<(), String>, cx: &mut Context<Self>) {
        self.secrets.error = result.as_ref().err().cloned();
        // Even a failed index save may have changed the OS store; agents
        // follow the in-memory vault either way.
        self.push_secrets_to_all();
        cx.notify();
    }

    pub(crate) fn handle_secrets_response(
        &mut self,
        ws_key: &str,
        id: u64,
        error: &Option<Value>,
        cx: &mut Context<Self>,
    ) -> bool {
        let pending = self.ws(ws_key).and_then(|ws| ws.pending.get(&id));
        if !matches!(pending, Some(Pending::Secrets)) {
            return false;
        }
        if let Some(ws) = self.ws_mut(ws_key) {
            ws.pending.remove(&id);
        }
        let unavailable = &mut self.secrets.unavailable;
        unavailable.retain(|key| key != ws_key);
        if let Some(error) = error {
            if error.get("code").and_then(Value::as_i64) == Some(METHOD_NOT_FOUND) {
                unavailable.push(ws_key.to_owned());
            } else {
                // The agent echoes only validation text, never values.
                let message = error.get("message").and_then(Value::as_str);
                self.secrets.error = Some(message.unwrap_or("rpc error").to_owned());
            }
        }
        cx.notify();
        true
    }
}
