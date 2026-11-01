//! Where secret values live (SECRETS.md): the OS secret store through the
//! `keyring` crate, or an in-memory map for tests. Service `GCode`, account
//! `secret:<NAME>` — fixed, so rebuilds and reinstalls find the same entries.

use std::sync::Arc;
#[cfg(test)]
use std::{collections::HashMap, sync::Mutex};

pub(crate) const SERVICE: &str = "GCode";

pub(crate) trait SecretStore: Send + Sync {
    /// `Ok(None)` when the entry does not exist.
    fn get(&self, name: &str) -> Result<Option<String>, String>;
    fn set(&self, name: &str, value: &str) -> Result<(), String>;
    /// Deleting a missing entry is not an error.
    fn delete(&self, name: &str) -> Result<(), String>;
}

pub(crate) fn account(name: &str) -> String {
    format!("secret:{name}")
}

/// The OS store, or `None` on platforms without one (the phone).
pub(crate) fn os_store() -> Option<Arc<dyn SecretStore>> {
    #[cfg(not(target_os = "android"))]
    {
        Some(Arc::new(KeyringStore))
    }
    #[cfg(target_os = "android")]
    {
        None
    }
}

#[cfg(not(target_os = "android"))]
struct KeyringStore;

#[cfg(not(target_os = "android"))]
impl KeyringStore {
    fn entry(name: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, &account(name)).map_err(|e| e.to_string())
    }
}

#[cfg(not(target_os = "android"))]
impl SecretStore for KeyringStore {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        match Self::entry(name)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        Self::entry(name)?
            .set_password(value)
            .map_err(|e| e.to_string())
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Manual check against the real OS store (`cargo test -- --ignored
/// os_store_round_trip`); uses its own service name so it never touches
/// GCode's entries.
#[cfg(all(test, not(target_os = "android")))]
#[test]
#[ignore]
fn os_store_round_trip() {
    let entry = keyring::Entry::new("GCode-selftest", "secret:ROUND_TRIP").unwrap();
    entry.set_password("round-trip-value").unwrap();
    assert_eq!(entry.get_password().unwrap(), "round-trip-value");
    entry.delete_credential().unwrap();
    assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
}

/// Test double: never touches the real OS store.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct MemoryStore {
    pub values: Mutex<HashMap<String, String>>,
    pub fail_writes: Mutex<bool>,
}

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        Ok(self.values.lock().unwrap().get(name).cloned())
    }

    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        if *self.fail_writes.lock().unwrap() {
            return Err("store refused the write".into());
        }
        self.values
            .lock()
            .unwrap()
            .insert(name.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        self.values.lock().unwrap().remove(name);
        Ok(())
    }
}
