//! Desktop remote scope: one per open phone socket. Filters what the phone
//! may send to the workspace CLI, injects the phone's connection identity,
//! remaps request ids into a range the desktop never uses, and recognises
//! the CLI's responses and frames that belong to the phone.
//!
//! The scope sits on raw stdio lines, where the CLI writes a subscribe ack
//! before that subscription's first frame, so ownership is known before any
//! frame arrives and nothing has to be held (REMOTE_CONTROL.md, LAN slice).

use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// First remote request id. Desktop ids count up from 1 and never get here;
/// 2^48 is still exact as a JS number on the CLI side.
pub(crate) const REMOTE_ID_BASE: u64 = 1 << 48;
static NEXT_REMOTE_ID: AtomicU64 = AtomicU64::new(REMOTE_ID_BASE);

pub(crate) const PHONE_CLIENT_MODE: &str = "web-remote-replayable";

const ALLOWED_METHODS: &[&str] = &[
    "v4/conversation/subscribe",
    "v4/conversation/unsubscribe",
    "v4/conversation/resync",
    "v4/conversation/rowsRange",
    "v4/conversation/attachmentRead",
    "v4/conversation/attachmentStat",
    "v4/connection/flow",
    "v4/attachment/begin",
    "v4/attachment/chunk",
    "v4/attachment/commit",
    "v4/attachment/abort",
    "v4/attachment/read",
    "v4/attachment/previewSource",
    "v4/command",
    // Read-only subagent directory for the phone header icons (REMOTE_CONTROL.md, Subagent icons).
    "session/subagents",
];

const ALLOWED_COMMANDS: &[&str] = &[
    "createSession",
    "sendText",
    "sendGoalCommand",
    "stop",
    "compact",
    "retryTurn",
    "editUserQuery",
    "setAssistantFeedback",
    "sendQueuedNow",
    "editQueueItem",
    "reorderQueueItem",
    "deleteQueueItem",
    "setAutoDrain",
    "resolveInteraction",
    "snoozeInteractionAutoResolution",
    "switchModelConfig",
    "switchCollaborationMode",
    "setFollowupMode",
    "pauseGoal",
    "resumeGoal",
    "cancelBackgroundWork",
    "renameSession",
];

/// Caller-supplied delivery fields the CLI would otherwise trust.
const STRIPPED_PARAMS: &[&str] = &["deliveryProfile", "subscriberScope", "workflowRunDeltas"];

pub(crate) fn next_remote_id() -> u64 {
    NEXT_REMOTE_ID.fetch_add(1, Ordering::Relaxed)
}

/// True for a response id in the remote range (any scope, live or closed).
pub(crate) fn is_remote_id(id: &Value) -> bool {
    id.as_u64().is_some_and(|n| n >= REMOTE_ID_BASE)
}

enum RequestKind {
    Plain,
    Subscribe(String),
}

struct PhoneRequest {
    phone_id: Value,
    kind: RequestKind,
}

/// What to do with one line from the phone.
#[derive(Debug, PartialEq)]
pub(crate) enum FromPhone {
    /// Rewritten line for the CLI.
    Forward(String),
    /// Answer for the phone; nothing reaches the CLI.
    Reply(String),
    /// Nothing to send (a rejected notification).
    Ignore,
    /// A model-catalog probe arrived before the desktop has its own catalog;
    /// answer it with `catalog_reply` once the desktop's probe returns.
    AwaitCatalog(Value),
}

/// The answer to a phone's `session/create` model-catalog probe: the
/// desktop's catalog, without a session id, so nothing is created.
pub(crate) fn catalog_reply(phone_id: Value, settings: &Value) -> String {
    json!({"id": phone_id, "result": {"settings": settings}}).to_string()
}

pub(crate) struct RemoteScope {
    pub(crate) connection_id: String,
    ws_key: String,
    requests: HashMap<u64, PhoneRequest>,
    /// subscriptionId → topic, for every subscription the phone created.
    owned: HashMap<String, String>,
}

/// `error.data` marker on allowlist refusals, so the phone can tell "this
/// surface is desktop-only" apart from a real failure.
pub(crate) const PHONE_SCOPE_REFUSAL: &str = "phoneScope";

fn rpc_error(id: Value, code: i64, message: &str) -> String {
    json!({"id": id, "error": {"code": code, "message": message}}).to_string()
}

fn refusal(id: Value, message: &str) -> String {
    json!({"id": id, "error": {
        "code": -32601, "message": message, "data": {PHONE_SCOPE_REFUSAL: true},
    }})
    .to_string()
}

/// True for an error object produced by `refusal`.
pub(crate) fn is_phone_scope_refusal(error: &Value) -> bool {
    error
        .pointer(&format!("/data/{PHONE_SCOPE_REFUSAL}"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

impl RemoteScope {
    pub(crate) fn new(ws_key: &str) -> Self {
        Self {
            connection_id: format!("gpui-lan-{}", uuid::Uuid::now_v7()),
            ws_key: ws_key.to_string(),
            requests: HashMap::new(),
            owned: HashMap::new(),
        }
    }

    fn check(&self, method: &str, params: &Map<String, Value>) -> Result<(), &'static str> {
        if !ALLOWED_METHODS.contains(&method) {
            return Err("not available from the phone");
        }
        let str_param = |key: &str| params.get(key).and_then(Value::as_str).unwrap_or("");
        match method {
            "v4/conversation/subscribe" => {
                let topic = str_param("topic");
                let allowed = (topic.starts_with("conversation/") && topic.len() > 13)
                    || topic == format!("sessions-index/{}", self.ws_key)
                    || topic == format!("workspace-config/{}", self.ws_key);
                if !allowed {
                    return Err("topic not available from the phone");
                }
            }
            "v4/conversation/unsubscribe" | "v4/conversation/resync"
                if !self.owned.contains_key(str_param("subscriptionId")) =>
            {
                return Err("unknown subscription");
            }
            "v4/command" if !ALLOWED_COMMANDS.contains(&str_param("type")) => {
                return Err("command not available from the phone");
            }
            _ => {}
        }
        Ok(())
    }

    /// Filter and rewrite one phone line. `model_settings` answers the
    /// phone's `session/create` model-catalog probe from the desktop's cache.
    pub(crate) fn filter_phone_line(
        &mut self,
        line: &str,
        model_settings: Option<&Value>,
    ) -> FromPhone {
        let Ok(Value::Object(mut msg)) = serde_json::from_str::<Value>(line) else {
            return FromPhone::Reply(rpc_error(Value::Null, -32700, "parse error"));
        };
        let phone_id = msg.get("id").cloned().filter(|id| !id.is_null());
        let reply = |code, message: &str| match &phone_id {
            Some(id) => FromPhone::Reply(rpc_error(id.clone(), code, message)),
            None => FromPhone::Ignore,
        };
        let Some(method) = msg
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            return reply(-32600, "only requests are accepted");
        };
        if method == "session/create" {
            return match (phone_id, model_settings) {
                (Some(id), Some(settings)) => FromPhone::Reply(catalog_reply(id, settings)),
                (Some(id), None) => FromPhone::AwaitCatalog(id),
                (None, _) => FromPhone::Ignore,
            };
        }
        let mut params = match msg.remove("params") {
            Some(Value::Object(params)) => params,
            None | Some(Value::Null) => Map::new(),
            Some(_) => return reply(-32602, "params must be an object"),
        };
        if let Err(message) = self.check(&method, &params) {
            return match phone_id {
                Some(id) => FromPhone::Reply(refusal(id, message)),
                None => FromPhone::Ignore,
            };
        }
        for key in STRIPPED_PARAMS {
            params.remove(*key);
        }
        let mut kind = RequestKind::Plain;
        if method == "v4/conversation/subscribe" {
            params.insert("clientMode".into(), json!(PHONE_CLIENT_MODE));
            params.insert("connectionId".into(), json!(self.connection_id));
            let topic = params.get("topic").and_then(Value::as_str).unwrap_or("");
            kind = RequestKind::Subscribe(topic.to_string());
        } else if params.contains_key("connectionId") || method == "v4/connection/flow" {
            params.insert("connectionId".into(), json!(self.connection_id));
        }
        msg.insert("params".into(), Value::Object(params));
        if let Some(phone_id) = phone_id {
            let id = next_remote_id();
            self.requests.insert(id, PhoneRequest { phone_id, kind });
            msg.insert("id".into(), json!(id));
        }
        FromPhone::Forward(Value::Object(msg).to_string())
    }

    pub(crate) fn owns_request(&self, id: u64) -> bool {
        self.requests.contains_key(&id)
    }

    /// Turn the CLI's response to one of this scope's requests back into the
    /// phone's id space. Subscribe acks record the phone's subscription.
    pub(crate) fn take_response(&mut self, id: u64, mut msg: Map<String, Value>) -> Option<String> {
        let request = self.requests.remove(&id)?;
        if let RequestKind::Subscribe(topic) = request.kind
            && let Some(sub) = msg
                .get("result")
                .and_then(|r| r.pointer("/ack/subscriptionId"))
                .and_then(Value::as_str)
        {
            self.owned.insert(sub.to_string(), topic);
        }
        msg.insert("id".into(), request.phone_id);
        Some(Value::Object(msg).to_string())
    }

    pub(crate) fn owns_subscription(&self, subscription_id: &str) -> bool {
        self.owned.contains_key(subscription_id)
    }

    /// Lines that release everything the phone held on the CLI, plus the
    /// subscription ids whose late frames must still be dropped.
    pub(crate) fn close(self) -> (Vec<String>, Vec<String>) {
        let mut lines = vec![
            json!({
                "id": next_remote_id(),
                "method": "v4/connection/flow",
                "params": {"connectionId": self.connection_id, "state": "closed"},
            })
            .to_string(),
        ];
        for (sub, topic) in &self.owned {
            lines.push(
                json!({
                    "id": next_remote_id(),
                    "method": "v4/conversation/unsubscribe",
                    "params": {
                        "topic": topic,
                        "subscriptionId": sub,
                        "connectionId": self.connection_id,
                    },
                })
                .to_string(),
            );
        }
        (lines, self.owned.into_keys().collect())
    }
}

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;
