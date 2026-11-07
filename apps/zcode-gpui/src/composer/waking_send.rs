//! Park a composer send while the workspace agent wakes up (WAKING_SEND.md).
//!
//! 修复原因：2026-11-07 项目 agent 空闲卸载后，第一次发送只会拉起 agent 并恢复草稿，
//! 下一次发送又在会话重新订阅前到达新 agent，被 CLI inbox 以 `proto.sessionNotFound`
//! 拒绝（未加载的会话 `getRevision` 为 null），而且没有任何路径重新订阅当前会话。
//! 这里按 auto-compact 已有的顺序（启动 → 订阅 → 发送）等待，并显示加载状态。

use crate::app::store::AppState;
use crate::backend::submission::Submission;
use crate::backend::workspace::Pending;
use crate::composer::attachment::AttachmentRef;
use gpui::Context;

pub(crate) struct WakingSend {
    submission: Submission,
    session: Option<String>,
    delivery: Option<String>,
    subscribe_sent: bool,
}

enum Step {
    Wait,
    Subscribe(String),
    Send,
    GiveUp(&'static str),
}

impl AppState {
    /// Whether a send to the current target must wait for the agent: it is
    /// starting, or the open conversation is not subscribed on it.
    pub(crate) fn send_needs_wake(&self, ws_key: &str) -> bool {
        let Some(ws) = self.ws(ws_key) else {
            return false;
        };
        if !(ws.started && ws.inbound.is_some()) {
            return ws.pumping;
        }
        match (&self.active, self.draft) {
            (Some(sid), false) => !self.conversations.get(sid).is_some_and(|c| c.subscribed),
            _ => false,
        }
    }

    pub(crate) fn park_waking_send(
        &mut self,
        ws_key: String,
        text: &str,
        attachments: Vec<AttachmentRef>,
        delivery: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let session = (!self.draft).then(|| self.active.clone()).flatten();
        let draft_key = self
            .active
            .clone()
            .unwrap_or_else(|| format!("draft:{}", self.active_workspace.as_deref().unwrap_or("")));
        if let Some(parked) = self.waking_send.as_mut()
            && parked.submission.workspace == ws_key
            && parked.session == session
        {
            let joined = &mut parked.submission.text;
            if !joined.is_empty() && !text.is_empty() {
                joined.push('\n');
            }
            joined.push_str(text);
            parked.submission.attachments.extend(attachments);
        } else {
            self.give_up_waking_send("you switched conversations", cx);
            let submission = Submission {
                workspace: ws_key,
                draft_key,
                navigation_generation: self.navigation_generation,
                text: text.to_owned(),
                attachments,
                config: self.submission_override(),
                model_choice: Some(self.current_model_choice()),
            };
            self.waking_send = Some(WakingSend {
                submission,
                session,
                delivery: delivery.map(str::to_owned),
                subscribe_sent: false,
            });
            self.push_log("agent is waking; the message sends when it is ready".into());
        }
        self.advance_waking_send(cx);
    }

    fn waking_step(&self) -> Option<(String, Step)> {
        let parked = self.waking_send.as_ref()?;
        let ws_key = parked.submission.workspace.clone();
        let on_target = self.active_ws_key().as_deref() == Some(ws_key.as_str())
            && (!self.draft).then(|| self.active.clone()).flatten() == parked.session;
        if !on_target {
            return Some((ws_key, Step::GiveUp("you switched conversations")));
        }
        let Some(ws) = self.ws(&ws_key) else {
            return Some((ws_key, Step::GiveUp("the project was removed")));
        };
        if ws.status == "backend down" || ws.status == "no backend" {
            return Some((ws_key, Step::GiveUp("the agent did not start")));
        }
        if !(ws.started && ws.inbound.is_some()) {
            return Some((ws_key, Step::Wait));
        }
        let Some(sid) = parked.session.clone() else {
            return Some((ws_key, Step::Send));
        };
        if self.conversations.get(&sid).is_some_and(|c| c.subscribed) {
            return Some((ws_key, Step::Send));
        }
        let subscribing = ws
            .pending
            .values()
            .any(|p| matches!(p, Pending::SubscribeConversation(s) if *s == sid));
        let step = match (subscribing, parked.subscribe_sent) {
            (true, _) => Step::Wait,
            (false, true) => Step::GiveUp("the conversation could not be loaded"),
            (false, false) => Step::Subscribe(sid),
        };
        Some((ws_key, step))
    }

    /// Run after every connection event: move the parked send one step.
    pub(crate) fn advance_waking_send(&mut self, cx: &mut Context<Self>) {
        let Some((ws_key, step)) = self.waking_step() else {
            return;
        };
        match step {
            Step::Wait => {}
            Step::Subscribe(sid) => {
                if let Some(parked) = self.waking_send.as_mut() {
                    parked.subscribe_sent = true;
                }
                self.subscribe_conversation(&ws_key, &sid);
            }
            Step::GiveUp(why) => self.give_up_waking_send(why, cx),
            Step::Send => {
                let Some(parked) = self.waking_send.take() else {
                    return;
                };
                let submission = parked.submission;
                let attachments = submission.attachments.clone();
                if !self.send_with_delivery(
                    &submission.text,
                    attachments,
                    parked.delivery.as_deref(),
                    None,
                    cx,
                ) {
                    self.recover_submission(submission, cx);
                }
                cx.notify();
            }
        }
    }

    /// Put the parked message back into its draft (no-op when nothing waits).
    pub(crate) fn give_up_waking_send(&mut self, why: &str, cx: &mut Context<Self>) {
        let Some(parked) = self.waking_send.take() else {
            return;
        };
        self.push_error(format!("Message not sent: {why}; draft restored"));
        self.recover_submission(parked.submission, cx);
        cx.notify();
    }

    /// The strip shows only while viewing the parked target.
    pub(crate) fn waking_send_visible(&self) -> bool {
        self.waking_send.is_some()
            && !matches!(self.waking_step(), Some((_, Step::GiveUp(_))) | None)
    }
}

impl crate::app::root::RootView {
    pub(crate) fn waking_send_feedback(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        use ely_gpui_component::buttons::Button;
        use ely_gpui_component::motion::{Spinner, SpinnerStyle};
        use gpui::{IntoElement, ParentElement, SharedString, Styled, div};
        if !self.state.read(cx).waking_send_visible() {
            return None;
        }
        let label = crate::shared::i18n::label;
        let cancel = Button::new("cancel-waking-send", label("Cancel", "取消"))
            .size(ely_gpui_component::theme::ControlSize::Sm)
            .on_click(cx.listener(|this, _, _, cx| {
                this.state
                    .update(cx, |s, cx| s.give_up_waking_send("cancelled", cx));
            }));
        let wrapper = div().child(cancel);
        #[cfg(test)]
        let wrapper =
            crate::app::test_support::track_children(wrapper, vec!["cancel-waking-send".into()]);
        Some(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_size(gpui::px(crate::shared::theme::font_size_sm()))
                .child(
                    Spinner::new("waking-send-spinner")
                        .style(SpinnerStyle::Ring)
                        .color(
                            crate::shared::theme_colors::color(crate::shared::theme::ACCENT).into(),
                        ),
                )
                .child(div().flex_1().child(SharedString::from(label(
                    "Waking the agent… your message sends when it is ready",
                    "正在唤醒智能体…就绪后自动发送",
                ))))
                .child(wrapper)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
#[path = "waking_send_tests.rs"]
mod tests;
