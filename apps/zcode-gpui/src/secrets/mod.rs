//! GPUI secrets vault (SECRETS.md). Values live in the OS secret store; the
//! names and their workspace scope live in `~/.zcode/v2/gpui_secrets.json`
//! (never values). `AppState.secrets` is the single writer; each workspace
//! agent gets its in-scope secrets with `secrets/sync` (backend/secrets_cmds.rs).

pub(crate) mod store;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use store::SecretStore;

/// Values shorter than this are not redacted by the agent (SECRETS.md).
pub(crate) const MIN_REDACTED_LEN: usize = 6;
const MAX_NAME_LEN: usize = 128;
/// Mirrors `isReservedSecretName` in packages/shared/src/zcode-protocol/index.ts.
const RESERVED: &[&str] = &[
    "PATH",
    "HOME",
    "USERPROFILE",
    "SHELL",
    "TEMP",
    "TMP",
    "TMPDIR",
    "COMSPEC",
    "SYSTEMROOT",
    "PATHEXT",
    "NODE_OPTIONS",
    "NODE_EXTRA_CA_CERTS",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SecretMeta {
    pub name: String,
    /// Workspace keys the secret is limited to; empty = every workspace.
    #[serde(default)]
    pub workspaces: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct IndexFile {
    #[serde(default)]
    secrets: Vec<SecretMeta>,
}

#[derive(Default)]
pub(crate) struct SecretVault {
    pub entries: Vec<SecretMeta>,
    /// Values read from the OS store; a name without a value is "missing".
    values: HashMap<String, String>,
    /// The startup read finished; syncs before that would send an empty table.
    pub loaded: bool,
    /// Workspaces whose agent answered method-not-found.
    pub unavailable: Vec<String>,
    pub error: Option<String>,
    index_path: Option<PathBuf>,
    store: Option<Arc<dyn SecretStore>>,
}

pub(crate) fn index_path() -> PathBuf {
    crate::shared::settings::resolve_user_home_dir()
        .join(".zcode")
        .join("v2")
        .join("gpui_secrets.json")
}

pub(crate) fn validate_name(name: &str) -> Result<(), &'static str> {
    let mut chars = name.chars();
    let valid = name.len() <= MAX_NAME_LEN
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid {
        return Err("Use letters, digits and _ only, not starting with a digit.");
    }
    let upper = name.to_ascii_uppercase();
    if RESERVED.contains(&upper.as_str())
        || upper.starts_with("ZCODE_")
        || upper.starts_with("GCODE_")
    {
        return Err("This name is reserved by the agent runtime.");
    }
    Ok(())
}

impl SecretVault {
    /// Desktop: the real index and OS store. Tests and the phone get an empty,
    /// storeless vault.
    pub(crate) fn for_app(remote: bool) -> Self {
        if cfg!(test) || remote {
            return Self::default();
        }
        match store::os_store() {
            Some(store) => Self::load(index_path(), store),
            None => Self::default(),
        }
    }

    pub(crate) fn load(index_path: PathBuf, store: Arc<dyn SecretStore>) -> Self {
        let entries = std::fs::read_to_string(&index_path)
            .ok()
            .and_then(|text| serde_json::from_str::<IndexFile>(&text).ok())
            .map(|index| index.secrets)
            .unwrap_or_default();
        Self {
            entries,
            index_path: Some(index_path),
            store: Some(store),
            ..Default::default()
        }
    }

    pub(crate) fn store(&self) -> Option<Arc<dyn SecretStore>> {
        self.store.clone()
    }

    pub(crate) fn has_value(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    /// Completes the startup read (values found, per-name read errors).
    pub(crate) fn finish_load(&mut self, values: HashMap<String, String>, error: Option<String>) {
        self.values = values;
        self.loaded = true;
        self.error = error;
    }

    /// The in-scope secrets with a value, sorted by name for a stable payload.
    pub(crate) fn secrets_for(&self, ws_key: &str) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .entries
            .iter()
            .filter(|meta| {
                meta.workspaces.is_empty() || meta.workspaces.iter().any(|k| k == ws_key)
            })
            .filter_map(|meta| {
                let value = self.values.get(&meta.name)?;
                Some((meta.name.clone(), value.clone()))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Add or replace: the OS store first, then the index. A failed store
    /// write leaves the index untouched.
    pub(crate) fn upsert(
        &mut self,
        name: &str,
        value: &str,
        workspaces: Vec<String>,
    ) -> Result<(), String> {
        let name = name.trim();
        validate_name(name).map_err(str::to_owned)?;
        if value.is_empty() {
            return Err("The value is empty.".into());
        }
        // Windows names are case-insensitive: replacing `token` must not add `TOKEN`.
        let existing = self
            .entries
            .iter()
            .position(|meta| meta.name.eq_ignore_ascii_case(name));
        if let Some(index) = existing
            && self.entries[index].name != name
        {
            return Err(format!(
                "A secret named {} already exists.",
                self.entries[index].name
            ));
        }
        let store = self.store.clone().ok_or("No OS secret store")?;
        store.set(name, value)?;
        self.values.insert(name.to_owned(), value.to_owned());
        match existing {
            Some(index) => self.entries[index].workspaces = workspaces,
            None => self.entries.push(SecretMeta {
                name: name.to_owned(),
                workspaces,
            }),
        }
        self.entries.sort_by(|a, b| a.name.cmp(&b.name));
        self.save_index()
    }

    pub(crate) fn remove(&mut self, name: &str) -> Result<(), String> {
        if let Some(store) = &self.store {
            store.delete(name)?;
        }
        self.values.remove(name);
        self.entries.retain(|meta| meta.name != name);
        self.save_index()
    }

    /// Adds or removes one workspace from a secret's scope.
    pub(crate) fn toggle_workspace(&mut self, name: &str, ws_key: &str) -> Result<(), String> {
        let Some(meta) = self.entries.iter_mut().find(|meta| meta.name == name) else {
            return Ok(());
        };
        match meta.workspaces.iter().position(|k| k == ws_key) {
            Some(index) => {
                meta.workspaces.remove(index);
            }
            None => meta.workspaces.push(ws_key.to_owned()),
        }
        self.save_index()
    }

    fn save_index(&self) -> Result<(), String> {
        let Some(path) = &self.index_path else {
            return Ok(());
        };
        let index = IndexFile {
            secrets: self.entries.clone(),
        };
        write_index(path, &index).map_err(|e| format!("Could not save the secrets list: {e}"))
    }
}

/// Reads every indexed value (blocking; run off the UI thread).
pub(crate) fn read_values(
    store: &dyn SecretStore,
    names: &[String],
) -> (HashMap<String, String>, Option<String>) {
    let mut values = HashMap::new();
    let mut error = None;
    for name in names {
        match store.get(name) {
            Ok(Some(value)) => {
                values.insert(name.clone(), value);
            }
            Ok(None) => {}
            Err(e) => error = Some(format!("Could not read {name}: {e}")),
        }
    }
    (values, error)
}

fn write_index(path: &Path, index: &IndexFile) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(index).map_err(std::io::Error::other)?;
    let staging = path.with_extension("json.tmp");
    std::fs::write(&staging, text)?;
    std::fs::rename(&staging, path)
}

#[cfg(test)]
mod tests;
