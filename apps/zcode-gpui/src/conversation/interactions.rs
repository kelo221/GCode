//! Pending interactions (permission requests, agent questions) mirrored from
//! the V4 conversation `pendingInteractions` region, and the
//! `resolveInteraction` answer shapes.
//!
//! Spec source: packages/shared/src/zcode-protocol-v4/snapshot.ts
//! (`pendingInteractionSchema`), command.ts (`resolveInteraction`), and the
//! answer mapping in apps/zcode-cli/packages/bootstrap/src/zcode-protocol/
//! interaction-broker.ts.
//!
//! Only the V4 command is used to answer. The backend also sends legacy
//! `interaction/requestPermission` / `requestUserInput` stdio requests and
//! races them against the V4 answer; the V4 answer cancels them.

use serde_json::{Map, Value, json};
use std::collections::HashMap;

/// `PERMISSION_FULL_ACCESS_OPTION_ID` in snapshot.ts.
pub const FULL_ACCESS_OPTION_ID: &str = "fullAccess";

#[derive(Clone, Debug, PartialEq)]
pub struct InteractionOption {
    pub option_id: String,
    pub label: String,
    /// allowOnce | allowAlways | deny | custom (permission); empty otherwise.
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QuestionOption {
    pub label: String,
    /// What is sent back; falls back to the label when the payload has none.
    pub value: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Question {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
}

/// `payload.schema.interaction` value the CLI uses for ExitPlanMode approval.
pub const PLAN_APPROVAL_INTERACTION: &str = "plan_approval";
/// `EXIT_PLAN_MODE_APPROVAL_APPROVE` in interaction-broker.ts.
pub const PLAN_APPROVE_VALUE: &str = "approve";

#[derive(Clone, Debug, PartialEq)]
pub struct PendingInteraction {
    pub interaction_id: String,
    /// permission | userInput | workspaceHookReview
    pub kind: String,
    pub tool_name: String,
    /// Permission summary or user-input prompt.
    pub text: String,
    pub options: Vec<InteractionOption>,
    pub full_access: Option<InteractionOption>,
    pub questions: Vec<Question>,
    pub free_text: bool,
    /// `payload.schema.interaction` (e.g. `plan_approval`); empty when absent.
    pub schema_interaction: String,
    /// ExitPlanMode `payload.input.plan` (markdown); `None` for other tools.
    pub plan: Option<String>,
}

fn str_of(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn parse_option(o: &Value) -> Option<InteractionOption> {
    Some(InteractionOption {
        option_id: o.get("optionId")?.as_str()?.to_string(),
        label: o.get("label")?.as_str()?.to_string(),
        kind: str_of(o, "kind"),
    })
}

impl PendingInteraction {
    pub fn from_value(v: &Value) -> Option<Self> {
        let interaction_id = v.get("interactionId")?.as_str()?.to_string();
        let payload = v.get("payload")?;
        let kind = v
            .get("kind")
            .or_else(|| payload.get("kind"))
            .and_then(Value::as_str)?
            .to_string();
        let text = match kind.as_str() {
            "permission" => str_of(payload, "summary"),
            _ => str_of(payload, "prompt"),
        };
        let options = payload
            .get("options")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(parse_option).collect())
            .unwrap_or_default();
        let questions = payload
            .get("questions")
            .and_then(Value::as_array)
            .map(|qs| {
                qs.iter()
                    .map(|q| Question {
                        question: str_of(q, "question"),
                        header: str_of(q, "header"),
                        options: q
                            .get("options")
                            .and_then(Value::as_array)
                            .map(|os| os.iter().map(parse_question_option).collect())
                            .unwrap_or_default(),
                        multi_select: q
                            .get("multiSelect")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            interaction_id,
            tool_name: str_of(payload, "toolName"),
            text,
            options,
            full_access: payload.get("fullAccessOption").and_then(parse_option),
            questions,
            free_text: payload
                .get("freeText")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            schema_interaction: payload
                .get("schema")
                .map(|s| str_of(s, "interaction"))
                .unwrap_or_default(),
            plan: payload
                .get("input")
                .and_then(|i| i.get("plan"))
                .and_then(Value::as_str)
                .map(str::to_string),
            kind,
        })
    }

    /// The option whose answer means "deny" (used for deny-with-feedback).
    pub fn deny_option(&self) -> Option<&InteractionOption> {
        self.options.iter().find(|o| o.kind == "deny")
    }

    pub fn is_plan_approval(&self) -> bool {
        self.kind == "userInput"
            && (self.schema_interaction == PLAN_APPROVAL_INTERACTION
                || self.tool_name.eq_ignore_ascii_case("ExitPlanMode"))
    }

    pub fn kind_class(&self) -> InteractionKind {
        InteractionKind::of(&self.kind)
    }

    /// For a kind GPUI cannot render: an option the payload itself offers to
    /// say "no" with. `None` means the card can only explain itself — a
    /// guessed answer could be ignored by the CLI (e.g. `pluginInstall`
    /// resolvers only listen to opted-in clients) and leave the card hidden
    /// while the agent keeps waiting.
    pub fn dismiss_option(&self) -> Option<&InteractionOption> {
        self.options.iter().find(|o| {
            o.kind == "deny"
                || ["decline", "cancel", "deny", "skip", "dismiss"]
                    .iter()
                    .any(|id| o.option_id.eq_ignore_ascii_case(id))
        })
    }
}

/// How the card renders a pending interaction (`pendingInteractionSchema.kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionKind {
    Permission,
    UserInput,
    WorkspaceHookReview,
    /// Kinds this client does not implement (upstream `pluginInstall`, future
    /// additions): a neutral "not supported" card, never another kind's text.
    Unsupported,
}

impl InteractionKind {
    pub fn of(kind: &str) -> Self {
        match kind {
            "permission" => Self::Permission,
            "userInput" => Self::UserInput,
            "workspaceHookReview" => Self::WorkspaceHookReview,
            _ => Self::Unsupported,
        }
    }
}

fn parse_question_option(o: &Value) -> QuestionOption {
    let label = str_of(o, "label");
    let value = o
        .get("value")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .map_or_else(|| label.clone(), str::to_string);
    QuestionOption {
        label,
        value,
        description: str_of(o, "description"),
    }
}

/// Click on a listed option (permission options, simple user-input choices,
/// and Full access).
pub fn option_answer(option_id: &str) -> Value {
    json!({ "optionId": option_id })
}

/// Free text, optionally attached to an option (deny with feedback).
pub fn free_text_answer(option_id: Option<&str>, text: &str) -> Value {
    let mut answer = json!({ "freeText": text });
    if let Some(id) = option_id {
        answer["optionId"] = json!(id);
    }
    answer
}

/// Question answer in the shape of Desktop `buildElicitationResponseContent`
/// (packages/ui/src/ElicitationDialog.tsx): `answers` keyed by question text
/// (comma-joined), plus `answer_<i>` per question and `answer` for a single
/// question. `picks` holds option *values*; `custom` the text typed per
/// question, appended after the picks (for single-select it replaces them,
/// as typing clears the pick in Desktop `updateCustomAnswer`).
///
// 修复：ExitPlanMode 的问题文本是投影层的 `reason`，与 broker 固定的问题文案不同，
// broker 只能经 `answer_0`/`answer` 取到审批值；此前只发 `answers[label]`（"Approve"
// 而非 value "approve"），每次批准都落成 "Permission denied for ExitPlanMode"。
pub fn questions_answer(
    questions: &[Question],
    picks: &HashMap<usize, Vec<String>>,
    custom: &HashMap<usize, String>,
) -> Value {
    let mut answers = Map::new();
    let mut content = Map::new();
    for (i, q) in questions.iter().enumerate() {
        let typed = custom.get(&i).map(|t| t.trim()).filter(|t| !t.is_empty());
        let mut values = match (q.multi_select, typed) {
            (false, Some(_)) => Vec::new(),
            _ => picks.get(&i).cloned().unwrap_or_default(),
        };
        values.extend(typed.map(str::to_string));
        if values.is_empty() {
            continue;
        }
        answers.insert(q.question.clone(), json!(values.join(", ")));
        let positional = if q.multi_select {
            json!(values)
        } else {
            json!(values[0])
        };
        if questions.len() == 1 {
            content.insert("answer".into(), positional.clone());
        }
        content.insert(format!("answer_{i}"), positional);
    }
    content.insert("answers".into(), Value::Object(answers));
    json!({ "action": "accept", "content": content })
}

/// ExitPlanMode approval. Non-empty `notes` ride along as
/// `content.approvalNotes`: the broker still approves, and the model gets
/// them as adjustments on top of the plan.
pub fn plan_approve_answer(pi: &PendingInteraction, notes: &str) -> Value {
    let questions = if pi.questions.is_empty() {
        vec![Question {
            question: pi.text.clone(),
            header: String::new(),
            options: vec![],
            multi_select: false,
        }]
    } else {
        pi.questions.clone()
    };
    let picks = HashMap::from([(0, vec![PLAN_APPROVE_VALUE.to_string()])]);
    let mut answer = questions_answer(&questions[..1], &picks, &HashMap::new());
    let notes = notes.trim();
    if !notes.is_empty() {
        answer["content"]["approvalNotes"] = json!(notes);
    }
    answer
}

pub fn decline_answer() -> Value {
    json!({ "action": "decline" })
}

/// Interactions answered by this client whose removal the backend has not
/// yet confirmed, keyed by `interactionId` (the request id). The backend
/// re-announces pending interactions (every second after a restore), so
/// without this set an answered card reappears and can be answered twice.
#[derive(Clone, Debug, Default)]
pub struct ResolvingInteractions(std::collections::HashSet<String>);

impl ResolvingInteractions {
    /// Claim an interaction for answering. `false` means an answer for this
    /// id is already in flight and nothing must be sent.
    pub fn begin(&mut self, interaction_id: &str) -> bool {
        self.0.insert(interaction_id.to_string())
    }

    /// The answer was rejected/failed: let the user answer again.
    pub fn release(&mut self, interaction_id: &str) {
        self.0.remove(interaction_id);
    }

    #[cfg(test)]
    pub fn contains(&self, interaction_id: &str) -> bool {
        self.0.contains(interaction_id)
    }

    /// Apply an authoritative `pendingInteractions` list: drop duplicates and
    /// cards with an answer in flight, and forget ids the backend no longer
    /// announces (it has settled them).
    pub fn reconcile(&mut self, incoming: Vec<PendingInteraction>) -> Vec<PendingInteraction> {
        self.0
            .retain(|id| incoming.iter().any(|pi| &pi.interaction_id == id));
        let mut seen = std::collections::HashSet::new();
        incoming
            .into_iter()
            .filter(|pi| !self.0.contains(&pi.interaction_id))
            .filter(|pi| seen.insert(pi.interaction_id.clone()))
            .collect()
    }
}

/// Toggle a pick (an option value): single-select replaces, multi-select
/// toggles membership.
pub fn toggle_pick(picks: &mut Vec<String>, value: &str, multi: bool) {
    if multi {
        if let Some(pos) = picks.iter().position(|p| p == value) {
            picks.remove(pos);
        } else {
            picks.push(value.to_string());
        }
    } else {
        *picks = vec![value.to_string()];
    }
}

/// Stepper position after Continue/Back, clamped to the question list.
pub fn step_question(current: usize, forward: bool, total: usize) -> usize {
    if forward {
        (current + 1).min(total.saturating_sub(1))
    } else {
        current.saturating_sub(1)
    }
}

#[cfg(test)]
#[path = "interactions_tests.rs"]
mod tests;
