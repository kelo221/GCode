use super::*;

const WS: &str = "C:\\work\\proj";

fn forward(scope: &mut RemoteScope, line: Value) -> Value {
    match scope.filter_phone_line(&line.to_string(), None) {
        FromPhone::Forward(out) => serde_json::from_str(&out).unwrap(),
        other => panic!("expected forward, got {other:?}"),
    }
}

fn reply(scope: &mut RemoteScope, line: Value) -> Value {
    match scope.filter_phone_line(&line.to_string(), None) {
        FromPhone::Reply(out) => serde_json::from_str(&out).unwrap(),
        other => panic!("expected reply, got {other:?}"),
    }
}

fn subscribe(scope: &mut RemoteScope, phone_id: u64, topic: &str, sub: &str) -> Value {
    let out = forward(
        scope,
        json!({"id": phone_id, "method": "v4/conversation/subscribe", "params": {
            "topic": topic, "connectionId": "phone-made-up", "clientMode": "desktop-continuous",
            "deliveryProfile": "continuous",
        }}),
    );
    let remote = out["id"].as_u64().unwrap();
    let response =
        json!({"id": remote, "result": {"ack": {"subscriptionId": sub, "logEpoch": "e"}}});
    let Value::Object(map) = response else {
        unreachable!()
    };
    let back = scope
        .take_response(remote, map)
        .expect("scope owns its request");
    serde_json::from_str(&back).unwrap()
}

#[test]
fn subscribe_injects_identity_and_mode_and_remaps_the_id() {
    let mut scope = RemoteScope::new(WS);
    let out = forward(
        &mut scope,
        json!({"id": 7, "method": "v4/conversation/subscribe", "params": {
            "topic": "conversation/s1", "connectionId": "spoofed", "clientMode": "desktop-continuous",
            "deliveryProfile": "continuous", "subscriberScope": "x",
        }}),
    );
    let params = &out["params"];
    assert_eq!(params["connectionId"], json!(scope.connection_id));
    assert_eq!(params["clientMode"], json!(PHONE_CLIENT_MODE));
    assert!(params.get("deliveryProfile").is_none());
    assert!(params.get("subscriberScope").is_none());
    let remote = out["id"].as_u64().unwrap();
    assert!(remote >= REMOTE_ID_BASE);
    assert!(is_remote_id(&out["id"]));
    assert!(scope.owns_request(remote));
}

#[test]
fn subscribe_ack_records_ownership_and_restores_the_phone_id() {
    let mut scope = RemoteScope::new(WS);
    let back = subscribe(&mut scope, 7, "conversation/s1", "sub-e-1");
    assert_eq!(back["id"], json!(7));
    assert_eq!(back["result"]["ack"]["subscriptionId"], json!("sub-e-1"));
    assert!(scope.owns_subscription("sub-e-1"));
    assert!(!scope.owns_subscription("sub-e-2"));
}

#[test]
fn only_the_opened_workspace_topics_are_allowed() {
    let mut scope = RemoteScope::new(WS);
    forward(
        &mut scope,
        json!({"id": 1, "method": "v4/conversation/subscribe",
               "params": {"topic": format!("sessions-index/{WS}")}}),
    );
    let err = reply(
        &mut scope,
        json!({"id": 2, "method": "v4/conversation/subscribe",
               "params": {"topic": "sessions-index/C:\\other"}}),
    );
    assert_eq!(err["id"], json!(2));
    assert_eq!(err["error"]["code"], json!(-32601));
}

#[test]
fn unlisted_methods_and_commands_are_rejected() {
    let mut scope = RemoteScope::new(WS);
    for method in ["plugins/list", "workflows/list", "mcp/list", "session/read"] {
        let err = reply(&mut scope, json!({"id": 3, "method": method, "params": {}}));
        assert_eq!(err["error"]["code"], json!(-32601), "{method}");
        assert!(is_phone_scope_refusal(&err["error"]), "{method}");
    }
    let parse = reply(&mut scope, json!({"id": 1, "result": {}}));
    assert!(!is_phone_scope_refusal(&parse["error"]));
    let err = reply(
        &mut scope,
        json!({"id": 4, "method": "v4/command", "params": {"type": "deleteSession"}}),
    );
    assert_eq!(err["error"]["code"], json!(-32601));
    let out = forward(
        &mut scope,
        json!({"id": 5, "method": "v4/command", "params": {"type": "sendText"}}),
    );
    assert_eq!(out["params"]["type"], json!("sendText"));
    // The read-only subagent directory feeds the phone header icons.
    let out = forward(
        &mut scope,
        json!({"id": 6, "method": "session/subagents", "params": {"sessionId": "s1", "endedLimit": 20}}),
    );
    assert_eq!(out["params"]["sessionId"], json!("s1"));
    // A rejected notification has nobody to answer.
    assert_eq!(
        scope.filter_phone_line(&json!({"method": "plugins/list"}).to_string(), None),
        FromPhone::Ignore
    );
}

#[test]
fn phone_cannot_touch_desktop_subscriptions() {
    let mut scope = RemoteScope::new(WS);
    subscribe(&mut scope, 1, "conversation/s1", "sub-phone");
    for method in ["v4/conversation/unsubscribe", "v4/conversation/resync"] {
        let err = reply(
            &mut scope,
            json!({"id": 9, "method": method,
                   "params": {"topic": "conversation/s1", "subscriptionId": "sub-desktop"}}),
        );
        assert_eq!(err["error"]["code"], json!(-32601), "{method}");
        let out = forward(
            &mut scope,
            json!({"id": 10, "method": method,
                   "params": {"topic": "conversation/s1", "subscriptionId": "sub-phone",
                              "connectionId": "spoofed"}}),
        );
        assert_eq!(out["params"]["connectionId"], json!(scope.connection_id));
    }
}

#[test]
fn flow_and_attachment_params_get_the_scope_connection() {
    let mut scope = RemoteScope::new(WS);
    let out = forward(
        &mut scope,
        json!({"id": 1, "method": "v4/connection/flow", "params": {"state": "saturated"}}),
    );
    assert_eq!(out["params"]["connectionId"], json!(scope.connection_id));
    let out = forward(
        &mut scope,
        json!({"id": 2, "method": "v4/attachment/begin",
               "params": {"connectionId": "phone", "uploadId": "u"}}),
    );
    assert_eq!(out["params"]["connectionId"], json!(scope.connection_id));
}

#[test]
fn model_catalog_probe_is_answered_from_the_desktop_cache() {
    let mut scope = RemoteScope::new(WS);
    let settings = json!({"models": [{"id": "m"}]});
    let FromPhone::Reply(out) = scope.filter_phone_line(
        &json!({"id": 4, "method": "session/create", "params": {}}).to_string(),
        Some(&settings),
    ) else {
        panic!("session/create must not reach the CLI");
    };
    let out: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(out["id"], json!(4));
    assert_eq!(out["result"]["settings"], settings);
    assert!(out["result"].get("sessionId").is_none());
    // Not cached yet: the gateway holds the probe instead of answering empty.
    assert_eq!(
        scope.filter_phone_line(
            &json!({"id": 5, "method": "session/create", "params": {}}).to_string(),
            None,
        ),
        FromPhone::AwaitCatalog(json!(5))
    );
}

#[test]
fn malformed_lines_get_errors() {
    let mut scope = RemoteScope::new(WS);
    assert!(matches!(
        scope.filter_phone_line("not json", None),
        FromPhone::Reply(_)
    ));
    let err = reply(&mut scope, json!({"id": 1, "result": {}}));
    assert_eq!(err["error"]["code"], json!(-32600));
    let err = reply(
        &mut scope,
        json!({"id": 1, "method": "v4/command", "params": [1]}),
    );
    assert_eq!(err["error"]["code"], json!(-32602));
}

#[test]
fn close_releases_flow_and_every_owned_subscription() {
    let mut scope = RemoteScope::new(WS);
    subscribe(&mut scope, 1, "conversation/a", "sub-a");
    subscribe(&mut scope, 2, &format!("sessions-index/{WS}"), "six-b");
    let connection = scope.connection_id.clone();
    let (lines, retired) = scope.close();
    let lines: Vec<Value> = lines
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["method"], json!("v4/connection/flow"));
    assert_eq!(lines[0]["params"]["state"], json!("closed"));
    let mut unsubscribed: Vec<&str> = lines[1..]
        .iter()
        .map(|l| {
            assert_eq!(l["method"], json!("v4/conversation/unsubscribe"));
            assert_eq!(l["params"]["connectionId"], json!(connection));
            assert!(is_remote_id(&l["id"]));
            l["params"]["subscriptionId"].as_str().unwrap()
        })
        .collect();
    unsubscribed.sort();
    assert_eq!(unsubscribed, vec!["six-b", "sub-a"]);
    let mut retired = retired;
    retired.sort();
    assert_eq!(retired, vec!["six-b", "sub-a"]);
}

#[test]
fn desktop_ids_are_never_remote() {
    assert!(!is_remote_id(&json!(1)));
    assert!(!is_remote_id(&json!(REMOTE_ID_BASE - 1)));
    assert!(!is_remote_id(&json!("server-1")));
    assert!(is_remote_id(&json!(REMOTE_ID_BASE)));
}
