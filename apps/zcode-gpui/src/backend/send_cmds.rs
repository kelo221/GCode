use crate::app::store::AppState;
use crate::backend::workspace::Pending;
use crate::composer::attachment::AttachmentRef;
use gpui::Context;
use serde_json::json;

impl AppState {
    #[allow(dead_code)]
    pub fn send(&mut self, text: &str, cx: &mut Context<Self>) {
        let _ = self.send_with_attachments(text, Vec::new(), cx);
    }

    pub fn send_with_attachments(
        &mut self,
        text: &str,
        attachments: Vec<AttachmentRef>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.send_with_delivery(text, attachments, None, None, cx)
    }

    pub(crate) fn send_with_delivery(
        &mut self,
        text: &str,
        attachments: Vec<AttachmentRef>,
        delivery: Option<&str>,
        held: Option<crate::composer::held_queue::HeldDecision>,
        cx: &mut Context<Self>,
    ) -> bool {
        let recovery_text = text.trim();
        let plan_task = match crate::composer::slash::classify_slash_command(recovery_text) {
            crate::composer::slash::SlashAction::Plan(task)
                if self.composer_intent
                    == crate::conversation::msg_actions::ComposerIntent::Send =>
            {
                Some(task)
            }
            _ => None,
        };
        if plan_task.is_some() && (!attachments.is_empty() || self.is_read_only_view()) {
            return false;
        }
        if let Some(task) = &plan_task {
            self.enable_plan_override();
            if task.is_empty() {
                return true;
            }
        }
        let text = plan_task.as_deref().unwrap_or(recovery_text);
        if text.is_empty() && attachments.is_empty() {
            return true;
        }
        if self.composer_intent != crate::conversation::msg_actions::ComposerIntent::Send {
            if !attachments.is_empty() {
                self.push_log("attachments apply to send, not edit/rename".into());
                return false;
            }
            self.submit_intent(text, cx);
            return true;
        }
        if plan_task.is_none() && text.starts_with('/') {
            match crate::composer::slash::classify_slash_command(text) {
                crate::composer::slash::SlashAction::Compact => {
                    if !attachments.is_empty() {
                        self.push_log("attachments apply to send, not /compact".into());
                        return false;
                    }
                    if self.active.is_some() {
                        return self.compact_session(cx);
                    }
                }
                crate::composer::slash::SlashAction::ResumeGoal => {
                    if !attachments.is_empty() {
                        return false;
                    }
                    if let Some((workspace, sid)) = self.active_ws_key().zip(self.active.clone()) {
                        return self.change_goal(&workspace, &sid, true, cx);
                    }
                }
                crate::composer::slash::SlashAction::Goal(desc) => {
                    if !attachments.is_empty() {
                        self.push_log("attachments apply to send, not /goal".into());
                        return false;
                    }
                    if self.active.is_some() {
                        return if held.is_some() {
                            self.send_goal_command_with_held(&desc, held, cx)
                        } else {
                            self.send_goal_command(&desc, cx)
                        };
                    }
                }
                crate::composer::slash::SlashAction::Plain(_)
                | crate::composer::slash::SlashAction::Plan(_) => {}
            }
        }
        let Some(ws_key) = self.active_ws_key() else {
            self.push_log("no workspace selected".into());
            return false;
        };
        self.ensure_spawned(&ws_key, cx);
        // A sleeping agent is woken and the send parked (WAKING_SEND.md); held
        // decisions need a live run, so they keep the old refusal.
        if held.is_none() && self.send_needs_wake(&ws_key) {
            self.park_waking_send(ws_key, recovery_text, attachments, delivery, cx);
            cx.notify();
            return true;
        }
        if !self
            .ws(&ws_key)
            .is_some_and(|w| w.started && w.inbound.is_some())
        {
            self.push_log("backend still starting — try again in a moment".into());
            cx.notify();
            return false;
        }
        let draft_key = self
            .active
            .clone()
            .unwrap_or_else(|| format!("draft:{}", self.active_workspace.as_deref().unwrap_or("")));
        let submission = crate::backend::submission::Submission {
            workspace: ws_key.clone(),
            draft_key: draft_key.clone(),
            navigation_generation: self.navigation_generation,
            text: recovery_text.to_owned(),
            attachments: attachments.clone(),
            config: self.submission_override(),
            model_choice: Some(self.current_model_choice()),
        };
        if self.draft || self.active.is_none() {
            let config = self.with_submission_override(self.draft_config());
            let mut first_input = self.with_submission_override(json!({"text":text}));
            if !attachments.is_empty() {
                first_input["attachments"] = json!(attachments);
            }
            let mut payload = json!({
                "workspaceId":self.ws(&ws_key).map(|w| w.key.clone()).unwrap_or_default(),
                "firstInput":first_input,
            });
            if !config.is_null() {
                payload["config"] = config;
            }
            if !self.send_command(
                &ws_key,
                None,
                "createSession",
                payload,
                None,
                Pending::CreateSession(submission),
            ) {
                return false;
            }
            self.push_log("creating session…".into());
        } else if let Some(sid) = self.active.clone() {
            let mut payload = json!({"text":text});
            if !attachments.is_empty() {
                payload["attachments"] = json!(attachments);
            }
            let delivery = delivery.or_else(|| {
                self.conversations
                    .get(&sid)
                    .filter(|c| c.config.followup_mode == "guide")
                    .map(|_| "guide")
            });
            if let Some(delivery) = delivery {
                payload["requestedDelivery"] = json!(delivery);
            }
            let pending = if let Some(held) = held {
                payload["heldQueueDisposition"] = json!(held.disposition.wire());
                payload["expectedHeldQueueItemIds"] = json!(held.receipt.queue_ids);
                Pending::HeldSend {
                    submission,
                    trigger: held.receipt.trigger,
                }
            } else {
                Pending::SendText(submission)
            };
            let payload = self.with_submission_override(payload);
            if !self.send_command(&ws_key, Some(sid), "sendText", payload, None, pending) {
                return false;
            }
        }
        self.draft_submission_overrides.remove(&draft_key);
        self.session_drafts.remove(&draft_key);
        cx.notify();
        true
    }
}
