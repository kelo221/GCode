//! Banner for a local agent that stopped on its own after startup.
//!
//! 修复原因：后端在运行中崩溃（含原生内存耗尽）时，GPUI 只在内存日志里写
//! "restarting"，运行中的任务和子代理被中断却没有任何可见提示，用户以为它们还在跑。
//! 现在由等待线程把退出码随 `ConnEvent::Exited` 带回，非 GPUI 主动结束时推一条横幅。

use crate::app::store::AppState;
use crate::backend::conn::BackendExit;

/// Exit code of a native out-of-memory abort (V8/Bun `FatalProcessOutOfMemory`
/// on Windows surfaces as this exception code).
pub(crate) const NATIVE_OOM_EXIT: u32 = 0xE000_0008;

/// `(English, Chinese)` banner text for an exit GPUI did not ask for; `None`
/// when GPUI killed the process itself.
pub(crate) fn unexpected_exit_text(exit: &BackendExit) -> Option<(String, String)> {
    if exit.killed_by_gpui {
        return None;
    }
    let lost_en = "Running tasks and subagents were interrupted.";
    let lost_zh = "运行中的任务和子代理已中断。";
    Some(match exit.code {
        Some(code) if code as u32 == NATIVE_OOM_EXIT => (
            format!("Agent stopped unexpectedly: the system ran out of memory. {lost_en}"),
            format!("Agent 意外停止：系统内存不足。{lost_zh}"),
        ),
        Some(code) => {
            let code = code_text(code);
            (
                format!("Agent stopped unexpectedly (exit code {code}). {lost_en}"),
                format!("Agent 意外停止（退出码 {code}）。{lost_zh}"),
            )
        }
        None => (
            format!("Agent stopped unexpectedly. {lost_en}"),
            format!("Agent 意外停止。{lost_zh}"),
        ),
    })
}

/// Windows crash codes read as NTSTATUS hex (`0xC0000005`); ordinary codes
/// stay decimal.
fn code_text(code: i32) -> String {
    if code < 0 {
        format!("0x{:08X}", code as u32)
    } else {
        code.to_string()
    }
}

impl AppState {
    /// Push the banner for a started agent that exited on its own. Repeated
    /// crashes of one restart loop show the same text once.
    pub(crate) fn notify_unexpected_exit(&mut self, exit: Option<BackendExit>) {
        let Some((en, zh)) = exit.as_ref().and_then(unexpected_exit_text) else {
            return;
        };
        let text = crate::shared::i18n::label(&en, &zh).to_string();
        if !self.errors.contains(&text) {
            self.push_error(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exit(code: Option<i32>, killed_by_gpui: bool) -> BackendExit {
        BackendExit {
            code,
            killed_by_gpui,
        }
    }

    #[test]
    fn native_oom_names_memory() {
        // 0xE0000008 as the i32 a Windows ExitStatus reports.
        let (en, zh) = unexpected_exit_text(&exit(Some(-536_870_904), false)).unwrap();
        assert_eq!(
            en,
            "Agent stopped unexpectedly: the system ran out of memory. Running tasks and subagents were interrupted."
        );
        assert!(zh.contains("内存不足"));
    }

    #[test]
    fn other_codes_are_generic_with_the_code() {
        let (en, _) = unexpected_exit_text(&exit(Some(3), false)).unwrap();
        assert_eq!(
            en,
            "Agent stopped unexpectedly (exit code 3). Running tasks and subagents were interrupted."
        );
        let (en, zh) = unexpected_exit_text(&exit(Some(0xC000_0005_u32 as i32), false)).unwrap();
        assert!(en.contains("(exit code 0xC0000005)"));
        assert!(zh.contains("退出码 0xC0000005"));
        let (en, _) = unexpected_exit_text(&exit(None, false)).unwrap();
        assert_eq!(
            en,
            "Agent stopped unexpectedly. Running tasks and subagents were interrupted."
        );
    }

    #[test]
    fn gpui_initiated_kills_show_nothing() {
        assert!(unexpected_exit_text(&exit(Some(-536_870_904), true)).is_none());
        assert!(unexpected_exit_text(&exit(Some(1), true)).is_none());
    }
}
