//! Host side of `interaction/requestCredentialCipher` (SECRETS.md "Agent
//! credentials"): the agent stores `~/.zcode/v2/credentials.json` values
//! (MCP OAuth tokens, account logins) encrypted with a key only the host
//! holds. A byte-compatible port of upstream Electron's
//! `packages/shared/src/credential-security/systemCredentialCipher.ts`, so
//! GCode and the ZCode desktop app read the same file and the same OS entry:
//! AES-256-GCM, a 32-byte key in the OS store under service
//! `ZCode Credential Encryption`, account = SHA-256 hex of the resolved file
//! path, values `enc:v2:<os|env>:<24-hex fingerprint>:<iv>.<tag>.<bytes>`.
//!
//! Not ported: the Linux private-file fallback for hosts without Secret
//! Service and the cross-process `.keyring` file lock (GCode is the only host
//! process; one in-process lock serializes key creation).

use base64::Engine as _;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub(crate) use super::credential_cipher_host::{CipherKeyStore, HostEnv, os_key_store};

const LEGACY_PREFIX: &str = "enc:v1:";
const SYSTEM_PREFIX: &str = "enc:v2:";
pub(crate) const SYSTEM_SERVICE: &str = "ZCode Credential Encryption";
const SOURCE_FILE: &str = "credential-source";
const OS_SOURCE_MARKER: &str = "os\n";
const KEY_BYTES: usize = 32;
const TAG_BYTES: usize = 16;
/// `credentialPlaintextSchema` / `credentialCiphertextSchema` limits.
const PLAINTEXT_MAX_BYTES: usize = 1_048_576;
const CIPHERTEXT_MAX_CHARS: usize = PLAINTEXT_MAX_BYTES.div_ceil(3) * 4 + 128;

pub(crate) const DECRYPT_FAILED: &str = "ZCODE_CREDENTIAL_DECRYPT_FAILED";
pub(crate) const UNAVAILABLE: &str = "CREDENTIAL_SECURE_STORAGE_UNAVAILABLE";
pub(crate) const INVALID_PAYLOAD: &str = "CREDENTIAL_CIPHER_INVALID_PAYLOAD";

/// Node's `Buffer.from(_, "base64url")` accepts padding and loose trailing bits.
const B64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::URL_SAFE,
    GeneralPurposeConfig::new()
        .with_encode_padding(false)
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CipherError {
    pub code: &'static str,
}

fn err(code: &'static str) -> CipherError {
    CipherError { code }
}

#[derive(Clone, PartialEq, Eq)]
enum Source {
    Os,
    Env,
}

#[derive(Clone)]
struct KeyMaterial {
    key: [u8; KEY_BYTES],
    source: Source,
    legacy_secret: Option<String>,
}

pub(crate) struct CredentialCipher {
    file_path: PathBuf,
    store: Option<Arc<dyn CipherKeyStore>>,
    env: HostEnv,
    /// Successful key loads only, so an unlocked store is retried next time.
    cache: Mutex<Option<KeyMaterial>>,
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0; 32];
    out.copy_from_slice(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref());
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn fingerprint(key: &[u8]) -> String {
    hex(&sha256(key))[..24].to_string()
}

/// `digest(resolve(filePath)).toString("hex")`: the OS entry's account.
pub(crate) fn account_for(file_path: &Path) -> String {
    hex(&sha256(file_path.to_string_lossy().as_bytes()))
}

fn seal(value: &str, key: &[u8; KEY_BYTES]) -> Result<String, CipherError> {
    let mut iv = [0u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut iv)
        .map_err(|_| err(UNAVAILABLE))?;
    let sealing =
        LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).map_err(|_| err(UNAVAILABLE))?);
    let mut bytes = value.as_bytes().to_vec();
    let tag = sealing
        .seal_in_place_separate_tag(Nonce::assume_unique_for_key(iv), Aad::empty(), &mut bytes)
        .map_err(|_| err(UNAVAILABLE))?;
    Ok([&iv[..], tag.as_ref(), &bytes]
        .map(|p| B64.encode(p))
        .join("."))
}

fn open(payload: &str, key: &[u8; KEY_BYTES]) -> Result<String, CipherError> {
    let parts: Vec<&str> = payload.split('.').collect();
    let url_safe = |p: &&str| {
        p.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    };
    if parts.len() != 3 || !parts.iter().all(url_safe) {
        return Err(err(DECRYPT_FAILED));
    }
    let decode = |p: &str| B64.decode(p).map_err(|_| err(DECRYPT_FAILED));
    let (iv, tag, bytes) = (decode(parts[0])?, decode(parts[1])?, decode(parts[2])?);
    let iv: [u8; NONCE_LEN] = iv.try_into().map_err(|_| err(DECRYPT_FAILED))?;
    if tag.len() != TAG_BYTES {
        return Err(err(DECRYPT_FAILED));
    }
    let opening =
        LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).map_err(|_| err(UNAVAILABLE))?);
    let mut in_out = [bytes, tag].concat();
    let plain = opening
        .open_in_place(Nonce::assume_unique_for_key(iv), Aad::empty(), &mut in_out)
        .map_err(|_| err(DECRYPT_FAILED))?;
    String::from_utf8(plain.to_vec()).map_err(|_| err(DECRYPT_FAILED))
}

fn read_optional(path: &Path) -> Result<Option<String>, CipherError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(err(UNAVAILABLE)),
    }
}

impl CredentialCipher {
    pub(crate) fn new(
        file_path: PathBuf,
        store: Option<Arc<dyn CipherKeyStore>>,
        env: HostEnv,
    ) -> Self {
        Self {
            file_path,
            store,
            env,
            cache: Mutex::new(None),
        }
    }

    fn legacy_keys(&self, legacy_secret: Option<&str>) -> Vec<[u8; KEY_BYTES]> {
        let env = &self.env;
        let fallback = format!(
            "zcode-credential-fallback:{}:{}:{}",
            env.platform, env.home, env.username
        );
        // 修复原因：Bun 的 os.userInfo() 在进程环境缺少 USERNAME 时返回 "unknown"（Node 走系统 API），
        // 本机 fork 后端写下的 enc:v1 值就用了这个种子；只用于解密，GCM 认证会拒绝错误密钥。
        let bun_fallback = format!(
            "zcode-credential-fallback:{}:{}:unknown",
            env.platform, env.home
        );
        let secret = legacy_secret.filter(|s| !s.is_empty());
        let trimmed = secret.map(str::trim).filter(|s| !s.is_empty());
        let mut seeds: Vec<&str> = Vec::new();
        for seed in [
            secret.unwrap_or(&fallback),
            trimmed.unwrap_or(&fallback),
            &fallback,
            &bun_fallback,
        ] {
            if !seeds.contains(&seed) {
                seeds.push(seed);
            }
        }
        seeds.into_iter().map(|s| sha256(s.as_bytes())).collect()
    }

    /// `resolveManagedSecret`: the explicit secret, the explicit file, or with
    /// `discover_default` the `credential-secret` file next to the store.
    fn managed_secret(&self, discover_default: bool) -> Result<Option<String>, CipherError> {
        if self.env.secret.is_some() && self.env.secret_file.is_some() {
            return Err(err(UNAVAILABLE));
        }
        if let Some(secret) = &self.env.secret {
            return Ok(Some(secret.clone()));
        }
        let path = match (&self.env.secret_file, discover_default) {
            (Some(explicit), _) => PathBuf::from(explicit),
            (None, true) => self.file_path.with_file_name("credential-secret"),
            (None, false) => return Ok(None),
        };
        let Some(value) = read_optional(&path)? else {
            return if self.env.secret_file.is_some() {
                Err(err(UNAVAILABLE))
            } else {
                Ok(None)
            };
        };
        #[cfg(unix)]
        if self.env.secret_file.is_none() {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .map_err(|_| err(UNAVAILABLE))?
                .permissions()
                .mode();
            if mode & 0o077 != 0 {
                return Err(err(UNAVAILABLE));
            }
        }
        if !(32..=4096).contains(&value.len()) {
            return Err(err(UNAVAILABLE));
        }
        Ok(Some(value))
    }

    fn env_key(secret: String) -> KeyMaterial {
        KeyMaterial {
            key: sha256(secret.as_bytes()),
            source: Source::Env,
            legacy_secret: Some(secret),
        }
    }

    /// `getKey(create)`: never makes a replacement key for a store that
    /// already depends on one (the source marker or `enc:v2` values say so).
    fn key(&self, create: bool) -> Result<KeyMaterial, CipherError> {
        if let Some(secret) = self.managed_secret(false)? {
            return Ok(Self::env_key(secret));
        }
        let mut cache = self.cache.lock().map_err(|_| err(UNAVAILABLE))?;
        if let Some(material) = cache.as_ref() {
            return Ok(material.clone());
        }
        let source_path = self.file_path.with_file_name(SOURCE_FILE);
        let pinned = read_optional(&source_path)?;
        if pinned.as_deref().is_some_and(|p| p != OS_SOURCE_MARKER) {
            return Err(err(UNAVAILABLE));
        }
        let raw = read_optional(&self.file_path)?.unwrap_or_default();
        let use_os = pinned.is_some() || raw.contains("enc:v2:os:");
        if !use_os {
            if let Some(secret) = self.managed_secret(true)? {
                return Ok(Self::env_key(secret));
            }
            if raw.contains("enc:v2:env:") {
                return Err(err(UNAVAILABLE));
            }
        }
        let unbound = create && !use_os && !raw.contains(SYSTEM_PREFIX);
        let store = self.store.as_ref().ok_or(err(UNAVAILABLE))?;
        let mut encoded = store.get().map_err(|_| err(UNAVAILABLE))?;
        if encoded.is_none() && unbound {
            let mut fresh = [0u8; KEY_BYTES];
            SystemRandom::new()
                .fill(&mut fresh)
                .map_err(|_| err(UNAVAILABLE))?;
            let value = B64.encode(fresh);
            store.set(&value).map_err(|_| err(UNAVAILABLE))?;
            if store.get().map_err(|_| err(UNAVAILABLE))?.as_deref() != Some(value.as_str()) {
                return Err(err(UNAVAILABLE));
            }
            encoded = Some(value);
        }
        let encoded = encoded.ok_or(err(UNAVAILABLE))?;
        let valid = encoded.len() == 43
            && encoded
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        let key: [u8; KEY_BYTES] = valid
            .then(|| B64.decode(&encoded).ok())
            .flatten()
            .and_then(|k| k.try_into().ok())
            .ok_or(err(UNAVAILABLE))?;
        if pinned.is_none() {
            write_private(&source_path, OS_SOURCE_MARKER)?;
        }
        let material = KeyMaterial {
            key,
            source: Source::Os,
            legacy_secret: None,
        };
        *cache = Some(material.clone());
        Ok(material)
    }

    pub(crate) fn encrypt(&self, value: &str) -> Result<String, CipherError> {
        if value.len() > PLAINTEXT_MAX_BYTES {
            return Err(err(INVALID_PAYLOAD));
        }
        let material = self.key(true)?;
        let source = if material.source == Source::Os {
            "os"
        } else {
            "env"
        };
        let sealed = seal(value, &material.key)?;
        Ok(format!(
            "{SYSTEM_PREFIX}{source}:{}:{sealed}",
            fingerprint(&material.key)
        ))
    }

    pub(crate) fn decrypt(&self, value: &str) -> Result<String, CipherError> {
        if value.len() > CIPHERTEXT_MAX_CHARS {
            return Err(err(INVALID_PAYLOAD));
        }
        let Some(rest) = value.strip_prefix(SYSTEM_PREFIX) else {
            if value.starts_with("enc:") && !value.starts_with(LEGACY_PREFIX) {
                return Err(err(DECRYPT_FAILED));
            }
            let material = self.key(true)?;
            let Some(payload) = value.strip_prefix(LEGACY_PREFIX) else {
                return Ok(value.to_string());
            };
            // Upstream passes `ZCODE_CREDENTIAL_SECRET: legacySecret`, so an OS key means the default seed only.
            let keys = self.legacy_keys(material.legacy_secret.as_deref());
            return keys
                .iter()
                .find_map(|k| open(payload, k).ok())
                .ok_or(err(DECRYPT_FAILED));
        };
        let parts: Vec<&str> = rest.split(':').collect();
        let fp_ok =
            |fp: &str| fp.len() == 24 && fp.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if parts.len() != 3
            || parts[2].is_empty()
            || !matches!(parts[0], "os" | "env")
            || !fp_ok(parts[1])
        {
            return Err(err(DECRYPT_FAILED));
        }
        let material = self.key(false)?;
        let source = if material.source == Source::Os {
            "os"
        } else {
            "env"
        };
        if parts[0] != source || fingerprint(&material.key) != parts[1] {
            return Err(err(UNAVAILABLE));
        }
        open(parts[2], &material.key)
    }
}

/// Writes the source marker through a temp file so a crash leaves no half file.
fn write_private(path: &Path, text: &str) -> Result<(), CipherError> {
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temp, text)
        .and_then(|()| std::fs::rename(&temp, path))
        .map_err(|_| {
            let _ = std::fs::remove_file(&temp);
            err(UNAVAILABLE)
        })
}

#[cfg(test)]
#[path = "credential_cipher_tests.rs"]
mod tests;
