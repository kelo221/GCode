//! Reverse RPC dispatch for incoming agent-to-client requests on the stdio wire.
//!
//! Unlike client-to-agent commands (where the frontend is the client), the backend
//! agent also makes reverse JSON-RPC requests to the host/client during execution
//! (e.g. runtime preferences, provider runtime auth headers, automations, browser).
//!
//! Each method in the protocol must be explicitly classified:
//!   - `Raced`: Handled via V4 topics (e.g. `interaction/requestPermission`), ignored on stdio.
//!   - `Served`: Handled directly by this client with a schema-compliant response.
//!   - `FastFail`: Deliberately failed fast with a clear, actionable reason.
//!   - `Unknown`: Unrecognized method returning JSON-RPC -32601.
//!
//! Response shapes mirror the strict zod schemas in
//! `packages/shared/src/zcode-protocol/index.ts`; the wire format mirrors the
//! desktop host (`zcodeProtocolClient.respond{,Error}`: `{id, result|error}`).

use serde_json::{Value, json};

pub enum ReverseRpcAction {
    /// Raced with V4 topic (e.g. `pendingInteractions` + `resolveInteraction`).
    /// Do not answer via stdio; let V4 resolution win.
    Raced,
    /// Send this JSON line back to the backend.
    Respond(String),
}

/// JSON-RPC internal error: the host understood the request but cannot serve it.
const INTERNAL_ERROR: i64 = -32603;
const METHOD_NOT_FOUND: i64 = -32601;

/// Automations and off-peak tasks are scheduled by the desktop host
/// (`desktopCronScheduler.ts`), whose store GPUI cannot see. Answering with
/// empty lists or `bound: false` would be a false fact: the CLI treats
/// `automation/checkTaskBinding` as an authorization boundary that must fail
/// closed when unknown, and `-32601` would trigger its legacy-list fallback.
const SCHEDULER_UNAVAILABLE: &str = "Automations and off-peak tasks are managed by the GCode desktop app; \
     they are not available in the GPUI client";

/// Bot-channel replies and topic attachments belong to the desktop host's
/// Feishu/Lark relay, which GPUI excludes; `-32601` would misreport them as
/// an outdated client rather than an unavailable host capability.
const CHANNEL_UNAVAILABLE: &str = "Bot channels and topic resources are managed by the GCode desktop app; \
     they are not available in the GPUI client";

fn respond_result(id: &Value, result: Value) -> ReverseRpcAction {
    ReverseRpcAction::Respond(json!({ "id": id, "result": result }).to_string())
}

fn respond_error(id: &Value, code: i64, message: &str) -> ReverseRpcAction {
    ReverseRpcAction::Respond(
        json!({ "id": id, "error": { "code": code, "message": message } }).to_string(),
    )
}

/// Desktop-only method families that must fail fast with `-32603`, including
/// members added upstream after this table was written.
fn is_host_scheduler_method(method: &str) -> bool {
    method.starts_with("automation/") || method.starts_with("offPeak/")
}

fn is_host_channel_method(method: &str) -> bool {
    method.starts_with("bot/") || method.starts_with("topic/resource/")
}

/// Committed host preferences that answer `session/requestRuntimePreferences`.
#[derive(Clone, Debug)]
pub struct HostPrefs {
    pub memory_enabled: bool,
    /// Desktop with Settings › Voice on: the main session gets the Speak tool.
    pub voice_enabled: bool,
    /// Settings › Code search on: sessions get the built-in fff FindFiles/SearchCode tools.
    pub code_search_enabled: bool,
    /// Settings › Diagnostics on: edits report language-server errors and agents get Diagnostics.
    pub lsp_diagnostics_enabled: bool,
    /// Settings › MCP Servers "Load MCP tools on demand": `auto` | `local` | `off`.
    pub tool_search_mode: &'static str,
    /// Provider cards' "Prompt cache": `{providerId: "1h" | "off"}`; missing means `5m`.
    pub prompt_cache_modes: Value,
}

impl Default for HostPrefs {
    fn default() -> Self {
        Self {
            memory_enabled: false,
            voice_enabled: false,
            code_search_enabled: false,
            lsp_diagnostics_enabled: false,
            tool_search_mode: "off",
            prompt_cache_modes: json!({}),
        }
    }
}

/// Dispatches an agent-to-client request based on the method name.
pub fn dispatch_reverse_rpc(
    id: &Value,
    method: &str,
    _params: &Value,
    prefs: HostPrefs,
) -> ReverseRpcAction {
    match method {
        "interaction/requestPermission" | "interaction/requestUserInput" => ReverseRpcAction::Raced,

        "interaction/requestProviderRuntimeHeaders" => respond_result(
            id,
            json!({
                "headersApplied": false,
                "errorMessage": "Zai account providers are not supported yet; configure an API-key provider"
            }),
        ),

        "interaction/requestOfficialMcpAuthHeaders" => respond_result(
            id,
            json!({ "ok": false, "reason": "official_auth_unavailable" }),
        ),

        "session/requestRuntimePreferences" => respond_result(
            id,
            json!({
                "askUserQuestionAutoResolutionEnabled": true,
                "nativeSearchEnhancementsEnabled": true,
                "memoryEnabled": prefs.memory_enabled,
                "voiceEnabled": prefs.voice_enabled,
                "codeSearchEnabled": prefs.code_search_enabled,
                "lspDiagnosticsEnabled": prefs.lsp_diagnostics_enabled,
                "toolSearchMode": prefs.tool_search_mode,
                "promptCacheModes": prefs.prompt_cache_modes
            }),
        ),

        // The desktop's VoiceService answers first (events.rs); reaching here
        // means no voice output (phone build or no service).
        "interaction/speak" => {
            respond_result(id, json!({ "accepted": false, "reason": "unavailable" }))
        }

        // No embedded browser: discovery reports none, execution fails with
        // the schema's `backend_unavailable` code instead of hanging.
        "interaction/browserList" => respond_result(id, json!({ "browsers": [] })),
        "interaction/browserExecute" => respond_result(
            id,
            json!({
                "ok": false,
                "error": {
                    "code": "backend_unavailable",
                    "message": "browser control is not available in the GPUI client"
                },
                "elapsedMs": 0
            }),
        ),

        m if is_host_scheduler_method(m) => {
            respond_error(id, INTERNAL_ERROR, SCHEDULER_UNAVAILABLE)
        }
        m if is_host_channel_method(m) => respond_error(id, INTERNAL_ERROR, CHANNEL_UNAVAILABLE),

        // Only sent when runtime preferences enable it, which GPUI never does;
        // mirrors Desktop's reply when it has no reader so the CLI keeps its
        // own Markdown profile loading.
        "subagents/readRuntimeConfig" => respond_error(
            id,
            INTERNAL_ERROR,
            "Subagent runtime config is unavailable on this Host",
        ),

        other => respond_error(
            id,
            METHOD_NOT_FOUND,
            &format!("Unsupported ZCode Protocol request: {other}"),
        ),
    }
}

#[cfg(test)]
#[path = "reverse_rpc_tests.rs"]
mod tests;
