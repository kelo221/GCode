//! Stopping a running subagent: the v3.18.2 `stopSubagent {taskId, executionId}`
//! command when the projection offers `runningSubagentSummary.stopControl`,
//! otherwise the older `cancelBackgroundWork {workId}` path.
//!
//! Spec sources: packages/shared/src/zcode-protocol-v4/command.ts
//! (`stopSubagent`, strict), snapshot.ts (`runningSubagentSummarySchema.stopControl`),
//! apps/zcode-cli/packages/bootstrap/src/zcode-protocol-v4/commands/handlers/
//! interaction-background.ts (`stopSubagent`, rejection reason codes).

use crate::app::root::RootView;
use crate::app::store::AppState;
use crate::backend::workspace::{CommandCtx, Pending};
use crate::conversation::model::ConversationState;
use crate::conversation::subagents::{JoinedRunningSubagent, SubagentStopControl};
use crate::shared::i18n::label;
use ely_gpui_component::buttons::{Button, ButtonVariant};
use ely_gpui_component::theme::ControlSize;
use gpui::{AnyElement, Context, prelude::*};
use serde_json::{Value, json};

/// Which command stops a subagent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubagentStop {
    /// `stopSubagent`: scoped to the exact execution the user is looking at.
    Execution(SubagentStopControl),
    /// `cancelBackgroundWork`: CLIs without `stopControl`.
    BackgroundWork(String),
}

impl SubagentStop {
    /// `(command type, payload)` for the V4 envelope.
    pub fn command(&self) -> (&'static str, Value) {
        match self {
            Self::Execution(c) => (
                "stopSubagent",
                json!({ "taskId": c.task_id, "executionId": c.execution_id }),
            ),
            Self::BackgroundWork(work_id) => ("cancelBackgroundWork", json!({ "workId": work_id })),
        }
    }
}

/// `stopControl` wins (upstream Desktop shows Stop only from it); without it
/// fall back to a cancellable background work id.
pub fn stop_target(
    stop_control: Option<&SubagentStopControl>,
    work_id: Option<&str>,
    cancellable: bool,
) -> Option<SubagentStop> {
    if let Some(control) = stop_control {
        return Some(SubagentStop::Execution(control.clone()));
    }
    work_id
        .filter(|id| cancellable && !id.trim().is_empty())
        .map(|id| SubagentStop::BackgroundWork(id.to_string()))
}

impl JoinedRunningSubagent {
    pub fn stop_target(&self) -> Option<SubagentStop> {
        stop_target(
            self.stop_control.as_ref(),
            self.work_id.as_deref(),
            self.cancellable,
        )
    }
}

impl ConversationState {
    /// Stop target for a transcript subagent row.
    pub fn subagent_stop_target(
        &self,
        work_id: Option<&str>,
        child_session_id: Option<&str>,
    ) -> Option<SubagentStop> {
        let control = child_session_id.and_then(|child| {
            self.subagents
                .as_ref()?
                .running
                .iter()
                .find(|r| r.child_session_id == child)?
                .stop_control
                .as_ref()
        });
        let (work_id, cancellable) = self.subagent_work_control(work_id, child_session_id);
        stop_target(control, work_id.as_deref(), cancellable)
    }
}

impl AppState {
    pub(crate) fn subagent_stop_pending(
        &self,
        workspace: &str,
        sid: &str,
        stop: &SubagentStop,
    ) -> bool {
        let (ctype, payload) = stop.command();
        self.ws(workspace).is_some_and(|ws| {
            ws.pending.values().any(|pending| {
                matches!(pending, Pending::Command(ctx)
                    if ctx.sid == sid && ctx.ctype == ctype && ctx.payload == payload)
            })
        })
    }

    pub(crate) fn stop_subagent_for(
        &mut self,
        workspace: &str,
        sid: &str,
        stop: &SubagentStop,
        cx: &mut Context<Self>,
    ) {
        if let SubagentStop::BackgroundWork(work_id) = stop {
            self.cancel_background_work_for(workspace, sid, work_id, cx);
            return;
        }
        if self.active_ws_key().as_deref() != Some(workspace)
            || self.active.as_deref() != Some(sid)
            || self.is_read_only_view()
            || self.subagent_stop_pending(workspace, sid, stop)
        {
            return;
        }
        // 与 cancelBackgroundWork 同规：在途请求是唯一状态，重复点击不重复 admission；
        // 被拒（执行已被替换/已结束）由 ack 的 reasonCode 走横幅说明。
        let (ctype, payload) = stop.command();
        self.send_session_command(workspace, CommandCtx::new(sid, ctype, payload));
        cx.notify();
    }
}

impl RootView {
    /// Stop button for a running subagent (dock card and transcript card).
    pub(crate) fn subagent_stop_button(
        &self,
        id: String,
        stop: SubagentStop,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.state.read(cx);
        let (workspace, parent) = state.active_ws_key().zip(state.active.clone())?;
        if state.is_read_only_view() {
            return None;
        }
        let pending = state.subagent_stop_pending(&workspace, &parent, &stop);
        Some(
            Button::new(gpui::SharedString::from(id), label("Stop", "停止"))
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .loading(pending)
                .disabled(pending)
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.state.update(cx, |state, cx| {
                        state.stop_subagent_for(&workspace, &parent, &stop, cx)
                    });
                }))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
#[path = "subagent_stop_tests.rs"]
mod tests;
