//! GPUI-only main-agent system prompt override (PARITY.md "System prompt
//! override"). The override lives in its own file, never in the shared
//! `setting.json` (Electron rewrites that file and drops unknown keys), and
//! is pushed to each workspace agent with `workspace/updateSystemPrompt`.
//! A missing file means the default prompt.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DefaultPrompt {
    pub text: String,
    /// `false` when the agent had no session yet and answered with the static
    /// identity text only.
    pub from_session: bool,
}

#[derive(Debug, Default)]
pub(crate) struct SystemPromptState {
    pub override_text: Option<String>,
    pub default_prompt: Option<DefaultPrompt>,
    pub reading: bool,
    /// The agent doesn't know the methods (e.g. the installed Desktop runtime).
    pub unavailable: bool,
    pub error: Option<String>,
    path: Option<PathBuf>,
}

pub(crate) fn override_path() -> PathBuf {
    crate::shared::settings::resolve_user_home_dir()
        .join(".zcode")
        .join("v2")
        .join("gpui_system_prompt.md")
}

impl SystemPromptState {
    /// Tests never read the real home file; the phone has no agents of its own.
    pub(crate) fn for_app(remote: bool) -> Self {
        if cfg!(test) || remote {
            Self::default()
        } else {
            Self::load(override_path())
        }
    }

    pub(crate) fn load(path: PathBuf) -> Self {
        let override_text = std::fs::read_to_string(&path)
            .ok()
            .filter(|text| !text.trim().is_empty());
        Self {
            override_text,
            path: Some(path),
            ..Default::default()
        }
    }

    /// Persists the new override (or deletes the file for the default) before
    /// it becomes the committed value.
    pub(crate) fn commit(&mut self, next: Option<String>) -> std::io::Result<()> {
        if let Some(path) = &self.path {
            write_override(path, next.as_deref())?;
        }
        self.override_text = next;
        self.error = None;
        Ok(())
    }
}

fn write_override(path: &Path, text: Option<&str>) -> std::io::Result<()> {
    let Some(text) = text else {
        return match std::fs::remove_file(path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let staging = path.with_extension("md.tmp");
    std::fs::write(&staging, text)?;
    std::fs::rename(&staging, path)
}

/// What Apply commits: blank text, or text equal to the default, means "no
/// override", so an untouched editor never freezes a copy of the default.
pub(crate) fn applied_override(editor: &str, default: Option<&str>) -> Option<String> {
    let text = editor.replace("\r\n", "\n");
    if text.trim().is_empty() {
        return None;
    }
    if default.is_some_and(|d| d.replace("\r\n", "\n").trim() == text.trim()) {
        return None;
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zcode_gpui_prompt_{name}_{}_{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn blank_or_default_text_applies_as_reset() {
        assert_eq!(applied_override("  \n", Some("Default")), None);
        assert_eq!(applied_override("Default\r\n", Some("Default")), None);
        assert_eq!(applied_override("Default", None), Some("Default".into()));
        assert_eq!(
            applied_override("Be brief.\r\nNo emoji.", Some("Default")),
            Some("Be brief.\nNo emoji.".into())
        );
    }

    #[test]
    fn commit_writes_and_reset_deletes_the_file() {
        let path = scratch("commit").join("v2").join("gpui_system_prompt.md");
        let mut state = SystemPromptState::load(path.clone());
        assert_eq!(state.override_text, None);

        state.commit(Some("Be brief.".into())).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "Be brief.");
        assert_eq!(
            SystemPromptState::load(path.clone())
                .override_text
                .as_deref(),
            Some("Be brief.")
        );

        state.commit(None).unwrap();
        assert!(!path.exists());
        state.commit(None).unwrap();
        assert_eq!(state.override_text, None);
    }

    #[test]
    fn a_blank_file_counts_as_the_default() {
        let dir = scratch("blank");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gpui_system_prompt.md");
        std::fs::write(&path, " \n").unwrap();
        assert_eq!(SystemPromptState::load(path).override_text, None);
    }
}
