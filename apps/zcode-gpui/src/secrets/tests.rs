use super::store::MemoryStore;
use super::*;

fn scratch() -> PathBuf {
    std::env::temp_dir()
        .join(format!("gpui_secrets_{}", uuid::Uuid::now_v7()))
        .join("gpui_secrets.json")
}

fn vault(path: &Path, store: &Arc<MemoryStore>) -> SecretVault {
    let mut vault = SecretVault::load(path.to_path_buf(), store.clone());
    let names: Vec<String> = vault.entries.iter().map(|m| m.name.clone()).collect();
    let (values, error) = read_values(store.as_ref(), &names);
    vault.finish_load(values, error);
    vault
}

#[test]
fn names_follow_the_protocol_rules() {
    assert!(validate_name("STEAM_TOKEN").is_ok());
    assert!(validate_name("_x1").is_ok());
    for bad in ["", "1A", "A-B", "A B", "ÄB"] {
        assert!(validate_name(bad).is_err(), "{bad}");
    }
    for reserved in [
        "path",
        "Http_Proxy",
        "ZCODE_HOME",
        "gcode_x",
        "NODE_OPTIONS",
    ] {
        assert!(validate_name(reserved).is_err(), "{reserved}");
    }
    assert!(validate_name(&"A".repeat(129)).is_err());
}

#[test]
fn values_go_to_the_store_and_only_names_to_the_index() {
    let path = scratch();
    let store = Arc::new(MemoryStore::default());
    let mut v = vault(&path, &store);
    v.upsert("STEAM_TOKEN", "steam-value-1", vec![]).unwrap();

    assert_eq!(
        store
            .values
            .lock()
            .unwrap()
            .get("STEAM_TOKEN")
            .map(String::as_str),
        Some("steam-value-1")
    );
    let index = std::fs::read_to_string(&path).unwrap();
    assert!(index.contains("STEAM_TOKEN"));
    assert!(!index.contains("steam-value-1"));

    // A restart (rebuild) reads the same entries back.
    let reloaded = vault(&path, &store);
    assert_eq!(
        reloaded.secrets_for("any"),
        vec![("STEAM_TOKEN".to_owned(), "steam-value-1".to_owned())]
    );
}

#[test]
fn scope_limits_secrets_to_listed_workspaces() {
    let store = Arc::new(MemoryStore::default());
    let mut v = vault(&scratch(), &store);
    v.upsert("GLOBAL", "global-value", vec![]).unwrap();
    v.upsert("GAME", "game-value-1", vec!["F:\\GoreGrind".into()])
        .unwrap();

    let names = |ws: &str| -> Vec<String> { v.secrets_for(ws).into_iter().map(|s| s.0).collect() };
    assert_eq!(names("F:\\GoreGrind"), vec!["GAME", "GLOBAL"]);
    assert_eq!(names("C:\\Other"), vec!["GLOBAL"]);

    v.toggle_workspace("GAME", "C:\\Other").unwrap();
    assert_eq!(v.secrets_for("C:\\Other").len(), 2);
    v.toggle_workspace("GAME", "C:\\Other").unwrap();
    v.toggle_workspace("GAME", "F:\\GoreGrind").unwrap();
    // No workspaces left = global again.
    assert_eq!(v.secrets_for("C:\\Other").len(), 2);
}

#[test]
fn replace_keeps_one_entry_and_case_variants_are_refused() {
    let store = Arc::new(MemoryStore::default());
    let mut v = vault(&scratch(), &store);
    v.upsert("TOKEN", "first-value", vec![]).unwrap();
    v.upsert("TOKEN", "second-value", vec![]).unwrap();
    assert_eq!(v.entries.len(), 1);
    assert_eq!(v.secrets_for("x")[0].1, "second-value");
    assert!(v.upsert("token", "third-value", vec![]).is_err());
    assert!(v.upsert("EMPTY", "", vec![]).is_err());
}

#[test]
fn a_failed_store_write_leaves_the_index_alone() {
    let path = scratch();
    let store = Arc::new(MemoryStore::default());
    let mut v = vault(&path, &store);
    *store.fail_writes.lock().unwrap() = true;
    assert!(v.upsert("TOKEN", "some-value", vec![]).is_err());
    assert!(v.entries.is_empty());
    assert!(!path.exists());
}

#[test]
fn delete_removes_value_and_name_and_missing_values_are_not_sent() {
    let store = Arc::new(MemoryStore::default());
    let path = scratch();
    let mut v = vault(&path, &store);
    v.upsert("A_TOKEN", "a-value-1", vec![]).unwrap();
    v.upsert("B_TOKEN", "b-value-1", vec![]).unwrap();
    v.remove("A_TOKEN").unwrap();
    assert!(!store.values.lock().unwrap().contains_key("A_TOKEN"));
    assert_eq!(v.entries.len(), 1);

    // An entry deleted outside GCode stays listed but is never injected.
    store.values.lock().unwrap().clear();
    let reloaded = vault(&path, &store);
    assert_eq!(reloaded.entries.len(), 1);
    assert!(!reloaded.has_value("B_TOKEN"));
    assert!(reloaded.secrets_for("x").is_empty());
}
