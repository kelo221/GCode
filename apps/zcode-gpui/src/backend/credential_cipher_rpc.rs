//! Answers `interaction/requestCredentialCipher` (SECRETS.md "Agent
//! credentials"). The agent encrypts `~/.zcode/v2/credentials.json` values only
//! through its host and has no local fallback, so a `-32601` here broke MCP
//! OAuth tokens and account logins. The OS store call can block, so it runs
//! on the background executor and the reply is sent when it finishes.

use crate::app::store::AppState;
use crate::secrets::credential_cipher::{
    CipherError, CredentialCipher, DECRYPT_FAILED, HostEnv, INVALID_PAYLOAD, os_key_store,
};
use gpui::{AppContext as _, Context};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};

pub(crate) const METHOD: &str = "interaction/requestCredentialCipher";

/// One cipher per credentials file, so its key cache outlives single requests.
fn cipher_for(file: PathBuf) -> Arc<CredentialCipher> {
    static CIPHERS: LazyLock<Mutex<HashMap<PathBuf, Arc<CredentialCipher>>>> =
        LazyLock::new(Default::default);
    let mut ciphers = CIPHERS.lock().unwrap_or_else(|e| e.into_inner());
    ciphers
        .entry(file.clone())
        .or_insert_with(|| {
            let store = os_key_store(&file);
            Arc::new(CredentialCipher::new(file, store, HostEnv::from_process()))
        })
        .clone()
}

/// The file the agent stores credentials in (`<home>/.zcode/v2/credentials.json`).
fn credentials_file() -> PathBuf {
    crate::shared::settings::settings_file_path().with_file_name("credentials.json")
}

/// The bridge's controlled error texts; native errors never cross the wire.
fn error_message(code: &str) -> String {
    match code {
        INVALID_PAYLOAD => "Invalid credential cipher payload".into(),
        DECRYPT_FAILED => "凭据解密失败：Invalid encrypted credential".into(),
        _ => "System credential storage is unavailable. Unlock the OS credential store and restart the application or CLI to retry.".into(),
    }
}

/// The `{requestId, result}` reply for one request; `None` when the params
/// are too malformed to correlate.
pub(crate) fn run(cipher: &CredentialCipher, params: &Value) -> Option<Value> {
    let request_id = params.get("requestId")?.as_str()?;
    let value = params.get("value").and_then(Value::as_str);
    let outcome: Result<String, CipherError> =
        match (params.get("operation").and_then(Value::as_str), value) {
            (Some("encrypt"), Some(value)) => cipher.encrypt(value),
            (Some("decrypt"), Some(value)) => cipher.decrypt(value),
            _ => Err(CipherError {
                code: INVALID_PAYLOAD,
            }),
        };
    let result = match outcome {
        Ok(value) => json!({ "ok": true, "value": value }),
        Err(e) => json!({
            "ok": false,
            "error": { "code": e.code, "message": error_message(e.code) }
        }),
    };
    Some(json!({ "requestId": request_id, "result": result }))
}

impl AppState {
    pub(crate) fn route_credential_cipher_rpc(
        &mut self,
        ws_key: &str,
        id: &Value,
        method: &str,
        params: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if method != METHOD {
            return false;
        }
        let (ws_key, id, params) = (ws_key.to_string(), id.clone(), params.clone());
        let cipher = cipher_for(credentials_file());
        cx.spawn(async move |this, cx| {
            let reply = cx
                .background_spawn(async move { run(&cipher, &params) })
                .await;
            let line = match reply {
                Some(result) => json!({ "id": id, "result": result }),
                None => json!({
                    "id": id,
                    "error": { "code": -32602, "message": "Invalid credential cipher request" }
                }),
            };
            let _ = this.update(cx, |state, _| {
                if let Some(ws) = state.ws_mut(&ws_key) {
                    ws.send_line(line.to_string());
                }
            });
        })
        .detach();
        true
    }
}
