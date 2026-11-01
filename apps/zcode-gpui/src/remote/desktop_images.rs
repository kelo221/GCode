//! Image bytes that live with the desktop: files the phone was shown
//! (`gcode/file/read`, phone only) and sent image attachments
//! (`v4/attachment/read`, desktop and phone). Both are read in chunks over
//! the workspace connection; the phone keeps finished images in a disk cache
//! under its temp dir (Android's app cache, which the system may clear).
//!
//! Cache names: `f-<sha(path)>-<mtime>-<size>` for desktop files, so an
//! edited file is fetched again, and `a-<sha(session, ref)>` for attachments,
//! whose content never changes.

use crate::app::store::AppState;
use crate::files::image_preview::MAX_IMAGE_BYTES;
use crate::remote::file_read::{FILE_READ_CHUNK, FILE_READ_METHOD};
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use gpui::{AppContext, AsyncApp, WeakEntity};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// `v4/attachment/read` allows at most 512 KiB per chunk.
const ATTACHMENT_CHUNK: u64 = 512 * 1024;
/// 20 MiB in 384 KiB chunks is 54 requests.
const MAX_CHUNKS: usize = 60;
const CACHE_MAX_BYTES: u64 = 200 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RemoteImage {
    DesktopFile(PathBuf),
    Attachment { session: String, reference: String },
}

impl RemoteImage {
    fn request(&self, offset: u64) -> (&'static str, Value) {
        match self {
            Self::DesktopFile(path) => (
                FILE_READ_METHOD,
                json!({"path": path.to_string_lossy(), "offset": offset, "limit": FILE_READ_CHUNK}),
            ),
            Self::Attachment { session, reference } => (
                "v4/attachment/read",
                json!({"sessionId": session, "ref": reference, "offset": offset,
                    "limit": ATTACHMENT_CHUNK}),
            ),
        }
    }
}

pub(crate) fn cache_dir() -> PathBuf {
    std::env::temp_dir().join("gcode-images")
}

fn digest(parts: &[&str]) -> String {
    let mut sha = Sha256::new();
    for part in parts {
        sha.update(part.as_bytes());
        sha.update([0]);
    }
    // sha2 0.11 digests no longer implement LowerHex; 16 bytes = 32 hex chars, as before.
    sha.finalize()[..16]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn file_cache_name(path: &Path, mtime: u64, size: u64) -> String {
    format!("f-{}-{mtime}-{size}", digest(&[&path.to_string_lossy()]))
}

fn attachment_cache_name(session: &str, reference: &str) -> String {
    format!("a-{}", digest(&[session, reference]))
}

/// One decoded answer chunk.
#[derive(Debug)]
pub(crate) struct Chunk {
    data: Vec<u8>,
    total: u64,
    /// `mtimeMs` for desktop files; attachments have none.
    stamp: u64,
    next: Option<u64>,
}

pub(crate) fn parse_chunk(value: &Value) -> Result<Chunk, String> {
    let bad = || "Malformed image data from the desktop".to_string();
    let data = value
        .get("dataBase64")
        .and_then(Value::as_str)
        .and_then(|s| BASE64_STANDARD.decode(s).ok())
        .ok_or_else(bad)?;
    let total = value
        .get("totalBytes")
        .and_then(Value::as_u64)
        .ok_or_else(bad)?;
    let next = match value.get("nextOffset") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().ok_or_else(bad)?),
    };
    Ok(Chunk {
        data,
        total,
        stamp: value.get("mtimeMs").and_then(Value::as_u64).unwrap_or(0),
        next,
    })
}

/// Joins chunks in order; a changed size or mtime between chunks means the
/// file changed mid-read, which must not produce a mixed image.
#[derive(Default)]
pub(crate) struct Assembly {
    bytes: Vec<u8>,
    first: Option<(u64, u64)>,
}

impl Assembly {
    /// The next offset to ask for, or `None` when complete.
    pub(crate) fn push(&mut self, chunk: Chunk) -> Result<Option<u64>, String> {
        let shape = (chunk.total, chunk.stamp);
        if *self.first.get_or_insert(shape) != shape {
            return Err("The image changed on the desktop while loading".into());
        }
        if chunk.total == 0 || chunk.total > MAX_IMAGE_BYTES {
            return Err("Image is empty or too large to preview".into());
        }
        self.bytes.extend_from_slice(&chunk.data);
        let have = self.bytes.len() as u64;
        match chunk.next {
            Some(next) if next == have && have < chunk.total && !chunk.data.is_empty() => {
                Ok(Some(next))
            }
            None if have == chunk.total => Ok(None),
            _ => Err("Image data from the desktop is incomplete".into()),
        }
    }
}

async fn read_cached(path: PathBuf, cx: &mut AsyncApp) -> Option<Vec<u8>> {
    cx.background_spawn(async move { std::fs::read(path).ok() })
        .await
        .filter(|bytes| !bytes.is_empty())
}

/// Fetch the whole image. `cache` is the phone's cache dir (`None` on the
/// desktop, which reads attachments through its own CLI every time).
pub(crate) async fn fetch(
    state: WeakEntity<AppState>,
    ws_key: String,
    source: RemoteImage,
    cache: Option<PathBuf>,
    cx: &mut AsyncApp,
) -> Result<Vec<u8>, String> {
    let mut cached_as = match (&source, &cache) {
        (RemoteImage::Attachment { session, reference }, Some(dir)) => {
            let path = dir.join(attachment_cache_name(session, reference));
            if let Some(bytes) = read_cached(path.clone(), cx).await {
                return Ok(bytes);
            }
            Some(path)
        }
        _ => None,
    };
    let mut assembly = Assembly::default();
    let mut offset = 0;
    for _ in 0..MAX_CHUNKS {
        let (method, params) = source.request(offset);
        let reply = state
            .update(cx, |s, _| s.raw_query(&ws_key, method, params))
            .map_err(|_| "Closed".to_string())?
            .await
            .map_err(|_| "Connection to the desktop was lost".to_string())??;
        let chunk = parse_chunk(&reply)?;
        if offset == 0
            && let (RemoteImage::DesktopFile(path), Some(dir)) = (&source, &cache)
        {
            let path = dir.join(file_cache_name(path, chunk.stamp, chunk.total));
            if let Some(bytes) = read_cached(path.clone(), cx).await
                && bytes.len() as u64 == chunk.total
            {
                return Ok(bytes);
            }
            cached_as = Some(path);
        }
        match assembly.push(chunk)? {
            Some(next) => offset = next,
            None => {
                let bytes = assembly.bytes;
                if let Some(path) = cached_as {
                    let copy = bytes.clone();
                    cx.background_spawn(async move { store(&path, &copy) })
                        .await;
                }
                return Ok(bytes);
            }
        }
    }
    Err("Image has too many chunks".into())
}

/// Best effort: a failed write only costs a refetch next time.
fn store(path: &Path, bytes: &[u8]) {
    static TRIMMED: std::sync::Once = std::sync::Once::new();
    let Some(dir) = path.parent() else { return };
    TRIMMED.call_once(|| trim(dir, CACHE_MAX_BYTES));
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let partial = path.with_extension("part");
    if std::fs::write(&partial, bytes).is_ok() && std::fs::rename(&partial, path).is_err() {
        let _ = std::fs::remove_file(&partial);
    }
}

/// Delete the least recently written files until `dir` holds at most `max`.
pub(crate) fn trim(dir: &Path, max: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<_> = entries
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok().filter(|m| m.is_file())?;
            Some((meta.modified().ok()?, meta.len(), e.path()))
        })
        .collect();
    files.sort();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    for (_, len, path) in files {
        if total <= max {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

#[cfg(test)]
#[path = "desktop_images_tests.rs"]
mod tests;
