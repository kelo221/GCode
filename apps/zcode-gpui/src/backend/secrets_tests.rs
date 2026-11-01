use crate::app::store::AppState;
use crate::secrets::SecretVault;
use crate::secrets::store::MemoryStore;
use gpui::{AppContext, TestAppContext};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

fn connect(s: &mut AppState) -> (String, Receiver<String>) {
    let key = s.workspaces[0].key.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    s.workspaces[0].inbound = Some(tx);
    s.workspaces[0].started = true;
    (key, rx)
}

fn next(rx: &Receiver<String>) -> Value {
    serde_json::from_str(&rx.try_recv().unwrap()).unwrap()
}

fn memory_vault() -> SecretVault {
    let path = std::env::temp_dir()
        .join(format!("gpui_secrets_cmds_{}", uuid::Uuid::now_v7()))
        .join("gpui_secrets.json");
    let mut vault = SecretVault::load(path, Arc::new(MemoryStore::default()));
    vault.finish_load(Default::default(), None);
    vault
}

#[gpui::test]
fn ready_sync_sends_only_in_scope_secrets(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let (key, rx) = connect(s);
        s.sync_secrets(&key);
        assert!(
            rx.try_recv().is_err(),
            "a fresh agent already has an empty vault"
        );

        s.secrets = memory_vault();
        s.save_secret("OTHER_ONLY", "other-value", vec!["elsewhere".into()], cx)
            .unwrap();
        assert!(
            next(&rx)["params"]["secrets"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        s.sync_secrets(&key);
        assert!(
            rx.try_recv().is_err(),
            "nothing in scope for this workspace"
        );

        s.save_secret("STEAM_TOKEN", "steam-value-1", vec![key.clone()], cx)
            .unwrap();
        let pushed = next(&rx);
        assert_eq!(pushed["method"], "secrets/sync");
        assert_eq!(pushed["params"]["workspace"]["workspaceKey"], key);
        assert_eq!(
            pushed["params"]["secrets"],
            json!([{ "name": "STEAM_TOKEN", "value": "steam-value-1" }])
        );
        let id = pushed["id"].as_u64().unwrap();
        s.handle_response(&key, id, Some(json!({"count": 1})), None, cx);
        assert!(!s.ws(&key).unwrap().pending.contains_key(&id));

        // Reconnect: the new agent gets the table again.
        s.sync_secrets(&key);
        assert_eq!(next(&rx)["params"]["secrets"][0]["name"], "STEAM_TOKEN");
    });
}

#[gpui::test]
fn nothing_is_sent_before_the_startup_read_or_to_stopped_agents(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let (key, rx) = connect(s);
        let mut vault = memory_vault();
        vault.upsert("TOKEN", "token-value", vec![]).unwrap();
        vault.loaded = false;
        s.secrets = vault;
        s.sync_secrets(&key);
        assert!(
            rx.try_recv().is_err(),
            "an empty table would wipe the agent"
        );

        s.secrets.loaded = true;
        s.workspaces[0].started = false;
        s.delete_secret("TOKEN", cx);
        assert!(rx.try_recv().is_err());
        assert!(s.secrets.entries.is_empty());
    });
}

#[gpui::test]
fn delete_pushes_an_empty_table_and_old_agents_are_flagged(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let (key, rx) = connect(s);
        s.secrets = memory_vault();
        s.save_secret("TOKEN", "token-value", vec![], cx).unwrap();
        let _ = next(&rx);
        s.delete_secret("TOKEN", cx);
        let pushed = next(&rx);
        assert!(pushed["params"]["secrets"].as_array().unwrap().is_empty());

        let error = json!({"code": -32601, "message": "Method not found"});
        s.handle_response(&key, pushed["id"].as_u64().unwrap(), None, Some(error), cx);
        assert_eq!(s.secrets.unavailable, vec![key.clone()]);
        assert_eq!(s.secrets.error, None);
    });
}
