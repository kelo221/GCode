//! The text input inside the blocking interaction card: the custom answer of
//! an agent question (Desktop `ElicitationDialog.tsx` custom answer row),
//! plan-approval feedback, and permission deny feedback (Desktop
//! `PermissionDialog.tsx`). One input is rebound to whatever the card shows;
//! each question keeps its own draft across Back/Continue.

use crate::app::root::RootView;
use crate::composer::input::{Composer, ComposerEvent};
use crate::conversation::interactions::{
    InteractionKind, PendingInteraction, free_text_answer, plan_approve_answer, questions_answer,
};
use crate::shared::i18n::{label, t};
use crate::shared::theme::{BORDER, CARD};
use crate::shared::theme_colors::color as rgb;
use gpui::{App, Context, Entity, Window, div, prelude::*};
use std::collections::HashMap;

pub(crate) struct InteractionInput {
    pub(crate) input: Entity<Composer>,
    /// (interactionId, question index) the input currently edits.
    bound: Option<(String, usize)>,
    /// Drafts of the questions not being edited: interactionId → index → text.
    drafts: HashMap<String, HashMap<usize, String>>,
}

impl InteractionInput {
    pub(crate) fn new(cx: &mut Context<RootView>) -> Self {
        let input = cx.new(|cx| {
            Composer::new_single_line(t("chat.elicitation.customAnswer.placeholder"), cx)
        });
        cx.subscribe(&input, |this, _, ev: &ComposerEvent, cx| {
            if matches!(ev, ComposerEvent::Submitted(_)) {
                this.interaction_input_enter(cx);
            }
        })
        .detach();
        // Typing over a single-select question un-highlights its picked option.
        cx.observe(&input, |_, _, cx| cx.notify()).detach();
        Self {
            input,
            bound: None,
            drafts: HashMap::new(),
        }
    }
}

/// The interaction shown in the composer's slot: Desktop shows only the first
/// permission/userInput request; hook reviews don't block the composer.
// 修复原因：未知 kind（如 pluginInstall）此前也会被当成阻塞交互而隐藏输入框，
// 卡片却没有可用的应答按钮，用户只能干等。现在只有权限与提问才阻塞输入框。
pub(crate) fn blocking(pending: &[PendingInteraction]) -> Option<&PendingInteraction> {
    pending.iter().find(|pi| is_blocking(pi))
}

pub(crate) fn is_blocking(pi: &PendingInteraction) -> bool {
    matches!(
        pi.kind_class(),
        InteractionKind::Permission | InteractionKind::UserInput
    )
}

fn takes_text(pi: &PendingInteraction) -> bool {
    match pi.kind.as_str() {
        "permission" => pi.free_text && pi.deny_option().is_some(),
        "userInput" => pi.is_plan_approval() || !pi.questions.is_empty() || pi.free_text,
        _ => false,
    }
}

fn placeholder(pi: &PendingInteraction) -> &'static str {
    if pi.kind == "permission" {
        t("chat.permission.feedback.placeholder")
    } else {
        t("chat.elicitation.customAnswer.placeholder")
    }
}

impl RootView {
    fn input_key(&self, pi: &PendingInteraction) -> (String, usize) {
        let last = pi.questions.len().saturating_sub(1);
        let qi = if pi.is_plan_approval() {
            0
        } else {
            self.question_steps
                .get(&pi.interaction_id)
                .copied()
                .unwrap_or(0)
                .min(last)
        };
        (pi.interaction_id.clone(), qi)
    }

    /// Rebind the input to what the card shows: park the old draft, load the
    /// new one, and move focus into the card (back to the composer once the
    /// card is gone). The phone never auto-focuses (soft keyboard rule).
    pub(crate) fn sync_interaction_input(
        &mut self,
        shown: Option<&PendingInteraction>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = shown
            .filter(|pi| takes_text(pi))
            .map(|pi| self.input_key(pi));
        if key == self.interaction_input.bound {
            return;
        }
        let input = self.interaction_input.input.clone();
        let had_focus = input.read(cx).focus.is_focused(window);
        if let Some((iid, qi)) = self.interaction_input.bound.take() {
            let text = input.read(cx).text().to_string();
            self.interaction_input
                .drafts
                .entry(iid)
                .or_default()
                .insert(qi, text);
        }
        let live = shown.map(|pi| pi.interaction_id.as_str());
        self.interaction_input
            .drafts
            .retain(|iid, _| Some(iid.as_str()) == live);
        let text = key
            .as_ref()
            .and_then(|(iid, qi)| self.interaction_input.drafts.get(iid)?.get(qi).cloned())
            .unwrap_or_default();
        let hint = shown.map(placeholder);
        input.update(cx, |c, cx| {
            c.set_text(&text);
            if let Some(hint) = hint {
                c.placeholder = hint;
            }
            cx.notify();
        });
        let focus_card = key.is_some();
        self.interaction_input.bound = key;
        if !self.desktop_tools(cx) {
            return;
        }
        let target = if focus_card {
            input.read(cx).focus.clone()
        } else if had_focus {
            self.state.read(cx).composer.read(cx).focus.clone()
        } else {
            return;
        };
        window.defer(cx, move |window, cx| window.focus(&target, cx));
    }

    fn bound_text(&self, iid: &str, qi: Option<usize>, cx: &App) -> Option<String> {
        let (b_iid, b_qi) = self.interaction_input.bound.as_ref()?;
        (b_iid == iid && qi.is_none_or(|qi| qi == *b_qi)).then(|| {
            self.interaction_input
                .input
                .read(cx)
                .text()
                .trim()
                .to_string()
        })
    }

    /// Text typed for question `qi` of `iid` right now (empty when none).
    pub(crate) fn typed_text(&self, iid: &str, qi: usize, cx: &App) -> String {
        self.bound_text(iid, Some(qi), cx).unwrap_or_else(|| {
            self.interaction_input
                .drafts
                .get(iid)
                .and_then(|d| d.get(&qi))
                .map(|t| t.trim().to_string())
                .unwrap_or_default()
        })
    }

    /// A single-select pick replaces typed text for that question.
    pub(crate) fn clear_typed_text(&mut self, iid: &str, qi: usize, cx: &mut Context<Self>) {
        if let Some(d) = self.interaction_input.drafts.get_mut(iid) {
            d.remove(&qi);
        }
        if self.bound_text(iid, Some(qi), cx).is_some() {
            self.interaction_input.input.update(cx, |c, cx| {
                c.set_text("");
                cx.notify();
            });
        }
    }

    /// Forget everything picked or typed for an interaction being answered.
    pub(crate) fn clear_interaction_drafts(&mut self, iid: &str, cx: &mut Context<Self>) {
        self.question_picks.remove(iid);
        self.question_steps.remove(iid);
        self.interaction_input.drafts.remove(iid);
        if self.bound_text(iid, None, cx).is_some() {
            self.interaction_input
                .input
                .update(cx, |c, _| c.set_text(""));
        }
    }

    /// Submit an agent question: picks plus typed text per question.
    pub(crate) fn submit_questions(&mut self, pi: &PendingInteraction, cx: &mut Context<Self>) {
        let iid = pi.interaction_id.clone();
        let picks = self.question_picks.get(&iid).cloned().unwrap_or_default();
        let typed: HashMap<usize, String> = (0..pi.questions.len())
            .map(|qi| (qi, self.typed_text(&iid, qi, cx)))
            .collect();
        self.clear_interaction_drafts(&iid, cx);
        let answer = questions_answer(&pi.questions, &picks, &typed);
        self.state
            .update(cx, |app, cx| app.resolve_interaction(&iid, answer, cx));
    }

    /// Approve a plan, carrying any typed text along as approval notes.
    // 修复：此前 Approve 先清空草稿再发送预先构造的固定答案，输入框里的调整意见被丢弃；
    // 现在在清空前读取输入，随批准一起发出。
    pub(crate) fn approve_plan(&mut self, pi: &PendingInteraction, cx: &mut Context<Self>) {
        let iid = pi.interaction_id.clone();
        let notes = self.typed_text(&iid, 0, cx);
        self.clear_interaction_drafts(&iid, cx);
        let answer = plan_approve_answer(pi, &notes);
        self.state
            .update(cx, |app, cx| app.resolve_interaction(&iid, answer, cx));
    }

    /// Send the typed text as the answer (free-text input), the plan change
    /// request, or the deny feedback. `false` when nothing was typed.
    pub(crate) fn send_typed_answer(
        &mut self,
        pi: &PendingInteraction,
        cx: &mut Context<Self>,
    ) -> bool {
        let iid = pi.interaction_id.clone();
        let text = self.typed_text(&iid, 0, cx);
        if text.is_empty() {
            return false;
        }
        let deny = (pi.kind == "permission")
            .then(|| pi.deny_option().map(|o| o.option_id.clone()))
            .flatten();
        self.clear_interaction_drafts(&iid, cx);
        let answer = free_text_answer(deny.as_deref(), &text);
        self.state
            .update(cx, |app, cx| app.resolve_interaction(&iid, answer, cx));
        true
    }

    /// Button variant of `send_typed_answer`: an empty input explains itself.
    pub(crate) fn send_typed_answer_or_hint(
        &mut self,
        pi: &PendingInteraction,
        cx: &mut Context<Self>,
    ) {
        if !self.send_typed_answer(pi, cx) {
            self.state.update(cx, |app, cx| {
                app.push_error(label("Type your answer first", "请先输入回答").into());
                cx.notify();
            });
        }
    }

    /// Enter in the input: next question / submit on the last one; elsewhere
    /// it sends typed text and ignores an empty input.
    fn interaction_input_enter(&mut self, cx: &mut Context<Self>) {
        let Some((iid, qi)) = self.interaction_input.bound.clone() else {
            return;
        };
        let pi = self.state.read(cx).active_conversation().and_then(|c| {
            c.pending_interactions
                .iter()
                .find(|p| p.interaction_id == iid)
                .cloned()
        });
        let Some(pi) = pi else {
            return;
        };
        if pi.is_plan_approval() || pi.questions.is_empty() {
            self.send_typed_answer(&pi, cx);
        } else if qi + 1 < pi.questions.len() {
            self.question_steps.insert(iid, qi + 1);
            cx.notify();
        } else {
            self.submit_questions(&pi, cx);
        }
    }

    /// The input row as drawn inside a card.
    pub(crate) fn interaction_input_row(&self) -> gpui::Div {
        div()
            .w_full()
            .min_w_0()
            .px_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(CARD))
            .child(self.interaction_input.input.clone())
    }
}

#[cfg(test)]
#[path = "interaction_input_tests.rs"]
mod tests;
