use super::*;

#[test]
fn test_raced_interactions_stay_unanswered() {
    let id = serde_json::json!("req-1");
    let params = serde_json::json!({});

    match dispatch_reverse_rpc(
        &id,
        "interaction/requestPermission",
        &params,
        HostPrefs::default(),
    ) {
        ReverseRpcAction::Raced => {}
        _ => panic!("interaction/requestPermission must be Raced"),
    }

    match dispatch_reverse_rpc(
        &id,
        "interaction/requestUserInput",
        &params,
        HostPrefs::default(),
    ) {
        ReverseRpcAction::Raced => {}
        _ => panic!("interaction/requestUserInput must be Raced"),
    }
}

#[test]
fn test_provider_runtime_headers_fast_fails() {
    let id = serde_json::json!("req-headers");
    let params = serde_json::json!({});
    match dispatch_reverse_rpc(
        &id,
        "interaction/requestProviderRuntimeHeaders",
        &params,
        HostPrefs::default(),
    ) {
        ReverseRpcAction::Respond(resp) => {
            let val: Value = serde_json::from_str(&resp).unwrap();
            assert_eq!(val["id"], "req-headers");
            assert_eq!(val["result"]["headersApplied"], false);
            assert!(
                val["result"]["errorMessage"]
                    .as_str()
                    .unwrap()
                    .contains("Zai account providers are not supported yet")
            );
        }
        _ => panic!("Must respond"),
    }
}

#[test]
fn test_runtime_preferences_served() {
    let id = serde_json::json!(42);
    let params = serde_json::json!({});
    for (enabled, tool_search_mode, prompt_cache_modes) in [
        (false, "off", serde_json::json!({})),
        (true, "auto", serde_json::json!({ "anthropic": "1h" })),
        (
            true,
            "local",
            serde_json::json!({ "anthropic": "1h", "proxy": "off" }),
        ),
    ] {
        match dispatch_reverse_rpc(
            &id,
            "session/requestRuntimePreferences",
            &params,
            HostPrefs {
                memory_enabled: enabled,
                voice_enabled: enabled,
                code_search_enabled: enabled,
                lsp_diagnostics_enabled: enabled,
                tool_search_mode,
                prompt_cache_modes: prompt_cache_modes.clone(),
            },
        ) {
            ReverseRpcAction::Respond(resp) => {
                let val: Value = serde_json::from_str(&resp).unwrap();
                assert_eq!(val["id"], 42);
                assert_eq!(val["result"]["askUserQuestionAutoResolutionEnabled"], true);
                assert_eq!(val["result"]["nativeSearchEnhancementsEnabled"], true);
                assert_eq!(val["result"]["memoryEnabled"], enabled);
                assert_eq!(val["result"]["voiceEnabled"], enabled);
                assert_eq!(val["result"]["codeSearchEnabled"], enabled);
                assert_eq!(val["result"]["lspDiagnosticsEnabled"], enabled);
                assert_eq!(val["result"]["toolSearchMode"], tool_search_mode);
                assert_eq!(val["result"]["promptCacheModes"], prompt_cache_modes);
            }
            _ => panic!("Must respond"),
        }
    }
}

#[test]
fn test_browser_and_mcp_requests() {
    let id = serde_json::json!("b1");
    let params = serde_json::json!({});

    match dispatch_reverse_rpc(
        &id,
        "interaction/browserList",
        &params,
        HostPrefs::default(),
    ) {
        ReverseRpcAction::Respond(resp) => {
            let val: Value = serde_json::from_str(&resp).unwrap();
            assert_eq!(val["result"]["browsers"].as_array().unwrap().len(), 0);
        }
        _ => panic!("Must respond"),
    }

    match dispatch_reverse_rpc(
        &id,
        "interaction/browserExecute",
        &params,
        HostPrefs::default(),
    ) {
        ReverseRpcAction::Respond(resp) => {
            let val: Value = serde_json::from_str(&resp).unwrap();
            assert_eq!(val["result"]["ok"], false);
            assert_eq!(val["result"]["error"]["code"], "backend_unavailable");
        }
        _ => panic!("Must respond"),
    }

    match dispatch_reverse_rpc(
        &id,
        "interaction/requestOfficialMcpAuthHeaders",
        &params,
        HostPrefs::default(),
    ) {
        ReverseRpcAction::Respond(resp) => {
            let val: Value = serde_json::from_str(&resp).unwrap();
            assert_eq!(val["result"]["ok"], false);
            assert_eq!(val["result"]["reason"], "official_auth_unavailable");
        }
        _ => panic!("Must respond"),
    }
}

#[test]
fn speak_without_voice_service_is_unavailable_not_unknown() {
    let val = respond("interaction/speak");
    assert_eq!(val["result"]["accepted"], false);
    assert_eq!(val["result"]["reason"], "unavailable");
}

fn respond(method: &str) -> Value {
    let id = serde_json::json!("auto-1");
    match dispatch_reverse_rpc(&id, method, &serde_json::json!({}), HostPrefs::default()) {
        ReverseRpcAction::Respond(resp) => serde_json::from_str(&resp).unwrap(),
        ReverseRpcAction::Raced => panic!("{method} must be answered"),
    }
}

/// Every scheduler method fails fast with -32603 and never reports a fake
/// fact: an empty list or `bound: false` would let the CLI's fail-closed
/// binding check pass, and -32601 would trigger its legacy-list fallback.
#[test]
fn test_automation_and_offpeak_fail_fast() {
    for method in [
        "automation/create",
        "automation/update",
        "automation/delete",
        "automation/list",
        "automation/checkTaskBinding",
        "offPeak/create",
        "offPeak/list",
        // Future members of the same desktop-owned families.
        "automation/pause",
        "offPeak/cancel",
    ] {
        let val = respond(method);
        assert_eq!(val["id"], "auto-1", "{method}");
        assert_eq!(val["error"]["code"], -32603, "{method}");
        assert!(
            val.get("result").is_none(),
            "{method} must not report a result"
        );
        assert!(
            val["error"]["message"]
                .as_str()
                .unwrap()
                .contains("GCode desktop"),
            "{method}"
        );
    }
}

/// Every agent-to-client method the CLI can send (`requestClient` call sites
/// in apps/zcode-cli bootstrap) is classified; none falls through to -32601.
#[test]
fn test_every_cli_reverse_request_is_classified() {
    for method in [
        "automation/create",
        "automation/update",
        "automation/checkTaskBinding",
        "automation/list",
        "automation/delete",
        "offPeak/create",
        "offPeak/list",
        "interaction/requestProviderRuntimeHeaders",
        "interaction/requestOfficialMcpAuthHeaders",
        "interaction/browserList",
        "interaction/browserExecute",
        "session/requestRuntimePreferences",
        "bot/channel/reply",
        "topic/resource/read",
        "topic/resource/cancel",
        "subagents/readRuntimeConfig",
    ] {
        let val = respond(method);
        assert_ne!(val["error"]["code"], -32601, "{method} must be classified");
    }
}

#[test]
fn test_bot_channel_and_topic_resources_fail_fast() {
    for method in [
        "bot/channel/reply",
        "topic/resource/read",
        "topic/resource/cancel",
    ] {
        let val = respond(method);
        assert_eq!(val["error"]["code"], -32603, "{method}");
        assert!(val.get("result").is_none(), "{method}");
        assert!(
            val["error"]["message"]
                .as_str()
                .unwrap()
                .contains("GCode desktop"),
            "{method}"
        );
    }
}

#[test]
fn test_unknown_method_returns_32601() {
    let val = respond("custom/unsupportedMethod");
    assert_eq!(val["error"]["code"], -32601);
    assert_eq!(
        val["error"]["message"],
        "Unsupported ZCode Protocol request: custom/unsupportedMethod"
    );
}
