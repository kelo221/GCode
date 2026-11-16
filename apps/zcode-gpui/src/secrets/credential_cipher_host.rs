//! Host inputs of the credential cipher (credential_cipher.rs): the OS store
//! entry that holds the key, and the process facts the legacy `enc:v1` keys
//! and the managed-secret override derive from. Split for the 400-line cap.

use super::credential_cipher::{SYSTEM_SERVICE, account_for};
use std::path::Path;
use std::sync::Arc;

/// The OS entry holding the base64url key.
pub(crate) trait CipherKeyStore: Send + Sync {
    fn get(&self) -> Result<Option<String>, String>;
    fn set(&self, encoded: &str) -> Result<(), String>;
}

/// What the legacy `enc:v1` keys and the managed-secret override derive from.
#[derive(Clone, Debug, Default)]
pub(crate) struct HostEnv {
    pub secret: Option<String>,
    pub secret_file: Option<String>,
    /// Node `os.platform()`: `win32` | `darwin` | `linux`.
    pub platform: String,
    pub home: String,
    pub username: String,
}

impl HostEnv {
    pub(crate) fn from_process() -> Self {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let platform = match std::env::consts::OS {
            "windows" => "win32",
            "macos" => "darwin",
            other => other,
        };
        // libuv: USERPROFILE / HOME, and the account name (`GetUserNameW` on Windows).
        let home = if cfg!(windows) {
            var("USERPROFILE")
        } else {
            var("HOME")
        };
        let username = login_name()
            .or_else(|| var("USER"))
            .or_else(|| var("LOGNAME"));
        Self {
            secret: var("ZCODE_CREDENTIAL_SECRET"),
            secret_file: var("ZCODE_CREDENTIAL_SECRET_FILE"),
            platform: platform.to_string(),
            home: home.unwrap_or_default(),
            username: username.unwrap_or_else(|| "unknown".into()),
        }
    }
}

#[cfg(windows)]
fn login_name() -> Option<String> {
    use windows::Win32::System::WindowsProgramming::GetUserNameW;
    let mut buf = [0u16; 257];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` holds `len` u16s; on success `len` counts the written chars plus the NUL.
    unsafe { GetUserNameW(Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len) }.ok()?;
    Some(String::from_utf16_lossy(
        &buf[..len.saturating_sub(1) as usize],
    ))
}

#[cfg(not(windows))]
fn login_name() -> Option<String> {
    None
}

/// The OS store entry, or `None` on the phone (no host credentials there).
pub(crate) fn os_key_store(file_path: &Path) -> Option<Arc<dyn CipherKeyStore>> {
    #[cfg(not(target_os = "android"))]
    {
        Some(Arc::new(KeyringKeyStore {
            account: account_for(file_path),
        }))
    }
    #[cfg(target_os = "android")]
    {
        let _ = file_path;
        None
    }
}

#[cfg(not(target_os = "android"))]
struct KeyringKeyStore {
    account: String,
}

#[cfg(not(target_os = "android"))]
impl CipherKeyStore for KeyringKeyStore {
    fn get(&self) -> Result<Option<String>, String> {
        let entry =
            keyring::Entry::new(SYSTEM_SERVICE, &self.account).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, encoded: &str) -> Result<(), String> {
        keyring::Entry::new(SYSTEM_SERVICE, &self.account)
            .and_then(|entry| entry.set_password(encoded))
            .map_err(|e| e.to_string())
    }
}
