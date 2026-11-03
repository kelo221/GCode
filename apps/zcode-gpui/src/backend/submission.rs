use crate::app::store::AppState;
use crate::composer::attachment::AttachmentRef;
use gpui::Context;
use serde_json::Value;

#[derive(Clone)]
pub(crate) struct Submission {
    pub workspace: String,
    pub draft_key: String,
    pub navigation_generation: u64,
    pub text: String,
    pub attachments: Vec<AttachmentRef>,
    pub config: Value,
    /// Model the message was sent with; remembered once the send is accepted.
    pub model_choice: Option<crate::shared::settings::LastComposerModel>,
}

pub(crate) fn ack_succeeded(ack: Option<&Value>) -> bool {
    matches!(
        ack.and_then(|a| a.get("status")).and_then(Value::as_str),
        Some("accepted" | "noop" | "duplicate")
    )
}

impl AppState {
    pub(crate) fn submit_composer(&mut self, cx: &mut Context<Self>) {
        self.submit_composer_with_trigger(crate::composer::delivery::SubmitTrigger::Ordinary, cx);
    }

    pub(crate) fn submit_composer_with_trigger(
        &mut self,
        trigger: crate::composer::delivery::SubmitTrigger,
        cx: &mut Context<Self>,
    ) {
        if self.block_upload_submission(cx) {
            return;
        }
        if self.prepare_plan_shortcut(cx) {
            return;
        }
        if self.begin_held_confirmation(trigger, cx) {
            return;
        }
        self.held_confirmation = None;
        let delivery = match self.submit_delivery(trigger, self.composer.read(cx).text()) {
            Ok(delivery) => delivery,
            Err(error) => {
                self.push_error(error.into());
                cx.notify();
                return;
            }
        };
        self.transfer_composer_submission(delivery, None, cx);
    }

    pub(crate) fn transfer_composer_submission(
        &mut self,
        delivery: Option<&str>,
        held: Option<crate::composer::held_queue::HeldDecision>,
        cx: &mut Context<Self>,
    ) {
        if self.block_upload_submission(cx) {
            return;
        }
        let (text, attachments, retired) = self
            .composer
            .update(cx, |composer, _| composer.take_submission());
        if self.send_with_delivery(&text, attachments.clone(), delivery, held, cx) {
            self.retired_temp_files.extend(retired);
        } else {
            // 入队失败仍由原 composer 持有附件及临时文件，不能提前转入退出清理队列。
            self.composer.update(cx, |composer, _| {
                composer.restore_submission(text, attachments, retired)
            });
        }
        cx.notify();
    }

    pub(crate) fn handle_submission_ack(
        &mut self,
        workspace: &str,
        submission: Submission,
        ack: Option<&Value>,
        create: bool,
        cx: &mut Context<Self>,
    ) {
        match ack.and_then(|a| a.get("status")).and_then(Value::as_str) {
            Some("rejected" | "failed" | "stale") => {
                // CLI 的 guard/stale 拒绝不写日志；之前横幅丢掉 reasonCode，事后无法判断原因。
                let line = rejection_line(ack);
                self.push_log(format!("submission {line}"));
                self.push_error(line);
                self.recover_submission(submission, cx);
            }
            Some("accepted" | "noop" | "duplicate") if create => {
                if let Some(choice) = submission.model_choice.clone() {
                    self.remember_composer_model(choice, cx);
                }
                let Some(sid) = ack
                    .and_then(|a| a.pointer("/result/sessionId"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                else {
                    // admission 可能已完成；缺失 sessionId 不等于拒绝，不能恢复成可重发消息。
                    self.push_error(
                        "Session creation outcome is unknown; check the task list before retrying"
                            .into(),
                    );
                    return;
                };
                if self.navigation_generation == submission.navigation_generation
                    && self.active_ws_key().as_deref() == Some(workspace)
                    && self.active.is_none()
                    && self.composer_intent
                        == crate::conversation::msg_actions::ComposerIntent::Send
                {
                    self.draft = false;
                    self.active = Some(sid.into());
                    self.ui_model_value = None;
                    self.ui_mode = None;
                    self.ui_plan = false;
                    self.ui_thought = None;
                }
                self.push_log("session created".into());
                self.subscribe_conversation(workspace, sid);
            }
            Some("accepted" | "noop" | "duplicate") => {
                if let Some(choice) = submission.model_choice {
                    self.remember_composer_model(choice, cx);
                }
            }
            _ => {
                self.push_error(
                    "Message admission is unknown; check the conversation before retrying".into(),
                );
            }
        }
    }

    pub(crate) fn save_recovered_submission(&mut self, submission: Submission) {
        if !submission.config.is_null() {
            self.draft_submission_overrides
                .entry(submission.draft_key.clone())
                .or_insert(submission.config);
        }
        let existing = self
            .session_drafts
            .get(&submission.draft_key)
            .cloned()
            .unwrap_or_default();
        self.session_drafts.insert(
            submission.draft_key.clone(),
            merge_text(&submission.text, &existing),
        );
        self.recovered_attachments
            .entry(submission.draft_key)
            .or_default()
            .extend(submission.attachments);
    }

    pub(crate) fn recover_submission(&mut self, submission: Submission, cx: &mut Context<Self>) {
        if !submission.config.is_null() {
            self.draft_submission_overrides
                .entry(submission.draft_key.clone())
                .or_insert(submission.config.clone());
        }
        // 明确拒绝才恢复未提交草稿；断线不自动重发，避免重复 admission。
        let current_key = self
            .active
            .clone()
            .unwrap_or_else(|| format!("draft:{}", self.active_workspace.as_deref().unwrap_or("")));
        if self.active_ws_key().as_deref() == Some(submission.workspace.as_str())
            && current_key == submission.draft_key
            && self.composer_intent == crate::conversation::msg_actions::ComposerIntent::Send
        {
            self.composer.update(cx, |composer, cx| {
                let existing = composer.text().to_owned();
                composer.set_text(&merge_text(&submission.text, &existing));
                for attachment in submission.attachments {
                    composer.add_attachment(attachment, cx);
                }
                cx.notify();
            });
        } else {
            self.save_recovered_submission(submission);
        }
    }
}

/// "Message was not accepted (rejected: guard.runtimeInitializing — msg); draft restored".
fn rejection_line(ack: Option<&Value>) -> String {
    let field = |key| {
        ack.and_then(|a| a.get(key))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    };
    let mut why = field("status").unwrap_or("rejected").to_owned();
    if let Some(code) = field("reasonCode") {
        why = format!("{why}: {code}");
    }
    if let Some(message) = field("message") {
        why = format!("{why} — {message}");
    }
    format!("Message was not accepted ({why}); draft restored")
}

fn merge_text(recovered: &str, current: &str) -> String {
    if current.is_empty() {
        recovered.to_owned()
    } else if recovered.is_empty() {
        current.to_owned()
    } else {
        format!("{recovered}\n{current}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn only_positive_ack_is_acceptance() {
        for status in ["rejected", "failed", "stale"] {
            assert!(!ack_succeeded(Some(&json!({"status": status}))));
        }
        assert!(!ack_succeeded(None));
        assert!(ack_succeeded(Some(&json!({"status":"accepted"}))));
        assert_eq!(merge_text("old", "new"), "old\nnew");
    }

    #[test]
    fn rejection_line_names_status_reason_and_message() {
        let ack = json!({"status": "rejected", "reasonCode": "guard.runtimeInitializing"});
        assert_eq!(
            rejection_line(Some(&ack)),
            "Message was not accepted (rejected: guard.runtimeInitializing); draft restored"
        );
        let ack = json!({"status": "failed", "reasonCode": "fault.command.executionFailed", "message": "boom"});
        assert_eq!(
            rejection_line(Some(&ack)),
            "Message was not accepted (failed: fault.command.executionFailed — boom); draft restored"
        );
        assert_eq!(
            rejection_line(Some(&json!({"status": "stale", "message": ""}))),
            "Message was not accepted (stale); draft restored"
        );
    }
}
