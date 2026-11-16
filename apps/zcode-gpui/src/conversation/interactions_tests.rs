//! Golden tests for `conversation/interactions.rs`: snapshot parsing and the
//! `resolveInteraction` answer shapes the CLI broker maps.

use super::*;

#[test]
fn parses_permission_with_full_access() {
    let v = json!({
        "interactionId": "req-1", "kind": "permission", "anchorRowId": 4, "createdAt": 1,
        "payload": {
            "kind": "permission", "toolCallId": "t1", "toolName": "Bash",
            "summary": "Run ls", "detail": null, "freeText": true,
            "options": [
                { "optionId": "allowOnce", "label": "Allow once", "kind": "allowOnce" },
                { "optionId": "allowAlways", "label": "Allow always", "kind": "allowAlways" },
                { "optionId": "deny", "label": "Deny", "kind": "deny" }
            ],
            "fullAccessOption": { "optionId": "fullAccess", "label": "Full access", "kind": "custom" }
        }
    });
    let pi = PendingInteraction::from_value(&v).unwrap();
    assert_eq!(pi.kind, "permission");
    assert_eq!(pi.tool_name, "Bash");
    assert_eq!(pi.text, "Run ls");
    assert_eq!(pi.options.len(), 3);
    assert!(pi.free_text);
    assert_eq!(pi.deny_option().unwrap().option_id, "deny");
    assert_eq!(
        pi.full_access.as_ref().unwrap().option_id,
        FULL_ACCESS_OPTION_ID
    );
}

#[test]
fn parses_ask_user_question() {
    let v = json!({
        "interactionId": "req-2", "kind": "userInput", "anchorRowId": null, "createdAt": 1,
        "payload": {
            "kind": "userInput", "prompt": "Pick", "freeText": false,
            "questions": [
                { "question": "Which DB?", "header": "DB",
                  "options": [ { "value": "pg", "label": "Postgres", "description": "SQL" },
                               { "value": "my", "label": "MySQL" } ] },
                { "question": "Extras?", "header": "Ext", "multiSelect": true,
                  "options": [ { "value": "a", "label": "Cache" }, { "value": "b", "label": "Queue" } ] }
            ]
        }
    });
    let pi = PendingInteraction::from_value(&v).unwrap();
    assert_eq!(pi.kind, "userInput");
    assert_eq!(pi.text, "Pick");
    assert!(pi.options.is_empty(), "no synthesized permission options");
    assert_eq!(pi.questions.len(), 2);
    assert_eq!(
        pi.questions[0].options[0],
        QuestionOption {
            label: "Postgres".into(),
            value: "pg".into(),
            description: "SQL".into()
        }
    );
    assert!(!pi.is_plan_approval());
    assert!(pi.questions[1].multi_select);
}

#[test]
fn answer_shapes_match_broker_mapping() {
    assert_eq!(
        option_answer("allowAlways"),
        json!({ "optionId": "allowAlways" })
    );
    assert_eq!(
        free_text_answer(Some("deny"), "use rg instead"),
        json!({ "optionId": "deny", "freeText": "use rg instead" })
    );
    assert_eq!(
        free_text_answer(None, "Postgres"),
        json!({ "freeText": "Postgres" })
    );
    assert_eq!(decline_answer(), json!({ "action": "decline" }));
}

#[test]
fn questions_answer_keys_by_question_text() {
    let questions = vec![
        Question {
            question: "Which DB?".into(),
            header: "DB".into(),
            options: vec![],
            multi_select: false,
        },
        Question {
            question: "Extras?".into(),
            header: "Ext".into(),
            options: vec![],
            multi_select: true,
        },
    ];
    let mut picks: HashMap<usize, Vec<String>> = HashMap::new();
    toggle_pick(picks.entry(0).or_default(), "my", false);
    toggle_pick(picks.entry(0).or_default(), "pg", false);
    toggle_pick(picks.entry(1).or_default(), "a", true);
    toggle_pick(picks.entry(1).or_default(), "b", true);
    toggle_pick(picks.entry(1).or_default(), "a", true);
    assert_eq!(
        questions_answer(&questions, &picks, &HashMap::new()),
        json!({ "action": "accept",
                "content": { "answers": { "Which DB?": "pg", "Extras?": "b" },
                             "answer_0": "pg", "answer_1": ["b"] } })
    );
}

#[test]
fn typed_answers_join_picks_and_replace_single_select_picks() {
    let q = |question: &str, multi_select| Question {
        question: question.into(),
        header: String::new(),
        options: vec![],
        multi_select,
    };
    let questions = vec![q("Which DB?", false), q("Extras?", true), q("Name?", false)];
    let picks = HashMap::from([(0, vec!["pg".to_string()]), (1, vec!["a".to_string()])]);
    let custom = HashMap::from([
        (0, "  sqlite  ".to_string()),
        (1, "c".to_string()),
        (2, "   ".to_string()),
    ]);
    assert_eq!(
        questions_answer(&questions, &picks, &custom),
        json!({ "action": "accept",
                "content": { "answers": { "Which DB?": "sqlite", "Extras?": "a, c" },
                             "answer_0": "sqlite", "answer_1": ["a", "c"] } })
    );
    let one = vec![q("Which DB?", false)];
    let custom = HashMap::from([(0, "duckdb".to_string())]);
    assert_eq!(
        questions_answer(&one, &HashMap::new(), &custom)["content"]["answer"],
        "duckdb"
    );
}

#[test]
fn queue_items_use_queue_item_id() {
    let q = crate::conversation::queue::QueueState::from_value(&json!({
        "autoDrain": false, "pauseReason": "stopped",
        "items": [ { "queueItemId": "q-1", "sourceCommandId": "c-1", "text": "next",
                     "dispatch": { "state": "queued" } } ]
    }))
    .unwrap();
    assert_eq!(q.items[0].queue_item_id, "q-1");
    assert!(!q.auto_drain);
    assert_eq!(q.pause_reason.as_deref(), Some("stopped"));
}

#[test]
fn active_phases() {
    assert!(crate::conversation::model::phase_is_active("running"));
    assert!(crate::conversation::model::phase_is_active("prewarming"));
    assert!(!crate::conversation::model::phase_is_active(
        "completedSuccess"
    ));
    assert!(!crate::conversation::model::phase_is_active("starting"));
}

fn pi(id: &str) -> PendingInteraction {
    PendingInteraction {
        interaction_id: id.into(),
        kind: "permission".into(),
        tool_name: "Bash".into(),
        text: String::new(),
        options: vec![],
        full_access: None,
        questions: vec![],
        free_text: false,
        schema_interaction: String::new(),
        plan: None,
    }
}

#[test]
fn resolving_is_idempotent_by_interaction_id() {
    let mut r = ResolvingInteractions::default();
    assert!(r.begin("req-1"));
    assert!(
        !r.begin("req-1"),
        "second answer for the same id is dropped"
    );
    assert!(r.begin("req-2"));
}

#[test]
fn reannounced_interaction_stays_hidden_while_answer_in_flight() {
    let mut r = ResolvingInteractions::default();
    r.begin("req-1");
    // Backend re-announces req-1 (restore loop) alongside a new req-2.
    let shown = r.reconcile(vec![pi("req-1"), pi("req-2"), pi("req-2")]);
    let ids: Vec<_> = shown.iter().map(|p| p.interaction_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["req-2"],
        "answered card hidden, duplicate collapsed"
    );
    assert!(r.contains("req-1"));
    // Backend settles req-1: the id is forgotten.
    r.reconcile(vec![pi("req-2")]);
    assert!(!r.contains("req-1"));
}

#[test]
fn released_interaction_becomes_answerable_again() {
    let mut r = ResolvingInteractions::default();
    r.begin("req-1");
    r.release("req-1");
    assert_eq!(r.reconcile(vec![pi("req-1")]).len(), 1);
    assert!(r.begin("req-1"));
}

fn plan_value() -> Value {
    json!({
        "interactionId": "req-plan", "kind": "userInput", "anchorRowId": 7, "createdAt": 1,
        "payload": {
            "kind": "userInput", "prompt": "Agent wants to exit plan mode", "freeText": true,
            "toolCallId": "t9", "toolName": "ExitPlanMode",
            "input": { "plan": "# Plan\n- step" },
            "schema": { "interaction": "plan_approval", "toolName": "ExitPlanMode" },
            "questions": [ { "question": "Agent wants to exit plan mode", "header": "Plan",
                "options": [ { "value": "approve", "label": "Approve",
                               "description": "Exit plan mode and start implementation." } ] } ]
        }
    })
}

#[test]
fn parses_plan_approval_with_plan_text() {
    let pi = PendingInteraction::from_value(&plan_value()).unwrap();
    assert!(pi.is_plan_approval());
    assert_eq!(pi.plan.as_deref(), Some("# Plan\n- step"));
    assert_eq!(pi.questions[0].options[0].value, PLAN_APPROVE_VALUE);
}

/// The broker (`normalizePlanApprovalAnswer`) reads
/// `answers[<fixed question>] ?? answer_0 ?? answer`; the projection's
/// question text differs from the fixed one, so the positional keys must
/// carry the value "approve" (not the label).
#[test]
fn plan_approval_answer_reaches_broker_through_positional_keys() {
    let pi = PendingInteraction::from_value(&plan_value()).unwrap();
    let answer = plan_approve_answer(&pi, "  ");
    assert_eq!(answer["action"], "accept");
    assert_eq!(answer["content"]["answer_0"], "approve");
    assert_eq!(answer["content"]["answer"], "approve");
    assert_eq!(
        answer["content"]["answers"]["Agent wants to exit plan mode"],
        "approve"
    );
    assert!(answer["content"].get("approvalNotes").is_none());
}

/// Text typed in the card goes along with the approval instead of being
/// dropped; the answer value stays "approve" so the broker still approves.
#[test]
fn plan_approval_carries_typed_notes() {
    let pi = PendingInteraction::from_value(&plan_value()).unwrap();
    let answer = plan_approve_answer(&pi, "  also add tests \n");
    assert_eq!(answer["content"]["answer_0"], "approve");
    assert_eq!(answer["content"]["approvalNotes"], "also add tests");
}

#[test]
fn option_value_falls_back_to_label() {
    let o = parse_question_option(&json!({ "label": "Cache" }));
    assert_eq!(o.value, "Cache");
}

#[test]
fn unknown_kinds_classify_as_unsupported() {
    assert_eq!(
        InteractionKind::of("permission"),
        InteractionKind::Permission
    );
    assert_eq!(InteractionKind::of("userInput"), InteractionKind::UserInput);
    assert_eq!(
        InteractionKind::of("workspaceHookReview"),
        InteractionKind::WorkspaceHookReview
    );
    for kind in ["pluginInstall", "somethingNew", ""] {
        assert_eq!(InteractionKind::of(kind), InteractionKind::Unsupported);
    }
}

/// Upstream v3.18.2 `pluginInstall` payload: parsed (not dropped), classed as
/// unsupported, and offers no guessed dismiss answer — it carries no options.
#[test]
fn plugin_install_is_unsupported_without_dismiss() {
    let v = json!({
        "interactionId": "plugin-install:s1:t1", "kind": "pluginInstall",
        "anchorRowId": null, "createdAt": 1,
        "payload": {
            "kind": "pluginInstall", "pluginId": "official/x", "name": "x",
            "reason": "Needs the x tools", "action": "install", "scope": "workspace",
            "toolCallId": "t1"
        }
    });
    let pi = PendingInteraction::from_value(&v).unwrap();
    assert_eq!(pi.kind_class(), InteractionKind::Unsupported);
    assert!(pi.dismiss_option().is_none());
}

#[test]
fn unsupported_kind_dismisses_with_its_own_decline_option() {
    let v = json!({
        "interactionId": "x1", "kind": "futureKind", "anchorRowId": null, "createdAt": 1,
        "payload": {
            "kind": "futureKind",
            "options": [
                { "optionId": "go", "label": "Go" },
                { "optionId": "cancel", "label": "Cancel" }
            ]
        }
    });
    let pi = PendingInteraction::from_value(&v).unwrap();
    assert_eq!(pi.kind_class(), InteractionKind::Unsupported);
    assert_eq!(pi.dismiss_option().unwrap().option_id, "cancel");
}

#[test]
fn stepper_clamps_to_question_range() {
    assert_eq!(step_question(0, true, 3), 1);
    assert_eq!(step_question(2, true, 3), 2);
    assert_eq!(step_question(0, false, 3), 0);
    assert_eq!(step_question(2, false, 3), 1);
}
