//! Tolerance of the conversation mirror for values added in upstream
//! v3.18.2 (diff a60ba6a7..upstream/main of packages/shared/src/zcode-protocol-v4):
//! new row kinds/markers, new optional fields and new interaction kinds must
//! neither panic nor drop the rest of the frame.

use super::*;
use crate::conversation::interactions::InteractionKind;
use serde_json::json;

/// One snapshot carrying every v3.18.2 addition GPUI does not model.
fn v3_18_2_snapshot() -> Value {
    json!({
        "logEpoch": "e1", "seq": 9, "revision": 4,
        "control": { "phase": "running" },
        // New optional session lock (rewindFiles) next to known keys.
        "availability": {
            "queueEdit": { "allowed": true },
            "rewindFiles": { "allowed": false, "reason": "busy" }
        },
        "queue": { "items": [{
            "queueItemId": "q1", "text": "next", "kind": "sendText",
            "dispatch": { "state": "queued" },
            "editLock": { "editorId": "ed-1", "clientId": "c-1", "inputOrigin": "mobile" }
        }]},
        "pendingInteractions": [{
            "interactionId": "plugin-install:s:t", "kind": "pluginInstall",
            "anchorRowId": null, "createdAt": 1,
            "payload": { "kind": "pluginInstall", "pluginId": "official/x", "name": "x",
                         "reason": "needs x", "action": "install", "scope": "workspace",
                         "toolCallId": "t" }
        }],
        "subagents": { "revision": 1, "childSessionIds": ["c1"], "endedTotal": 0, "running": [{
            "childSessionId": "c1", "subagentType": "Explore", "title": "t", "status": "running",
            "stopControl": { "taskId": "task", "executionId": "exec" }
        }]},
        "rows": { "firstRowId": 1, "totalCount": 6, "window": [
            { "rowId": 1, "turnId": "t1", "kind": "turnHeader", "state": "running",
              "createdAt": 1, "createdAtSeq": 1, "origin": "userInput" },
            { "rowId": 2, "turnId": "t1", "kind": "userInput", "text": "hi", "origin": "realUser",
              "createdAt": 1, "createdAtSeq": 2,
              "modelSelection": { "providerId": "p", "modelId": "m" },
              "mode": "build", "planEnabled": false },
            { "rowId": 3, "turnId": "t1", "kind": "timelineMarker", "createdAt": 1, "createdAtSeq": 3,
              "marker": { "type": "worktreeCreated", "path": "C:/wt", "sha": "abc",
                          "ref": null, "output": "Preparing worktree" } },
            // Consumer-synthesized placeholder shape, should a peer ever forward one.
            { "rowId": 4, "turnId": "t1", "kind": "unreadable", "sourceKind": "toolCall",
              "createdAt": 1, "createdAtSeq": 4 },
            // A kind this client has never heard of.
            { "rowId": 5, "turnId": "t1", "kind": "brandNewRow", "createdAt": 1,
              "createdAtSeq": 5, "extra": { "nested": [1, 2] } },
            { "rowId": 6, "turnId": "t1", "kind": "assistantText", "text": "still here",
              "state": "final", "createdAt": 1, "createdAtSeq": 6 }
        ]}
    })
}

#[test]
fn snapshot_with_new_values_keeps_every_row_and_region() {
    let mut c = ConversationState::default();
    c.apply_snapshot(&v3_18_2_snapshot());
    assert_eq!(c.revision, 4);
    assert_eq!(c.rows.len(), 6, "no row is dropped");
    assert!(matches!(c.rows[&3], Row::Other));
    assert!(matches!(c.rows[&4], Row::Other));
    assert!(matches!(c.rows[&5], Row::Other));
    assert!(matches!(&c.rows[&6], Row::AssistantText { text, .. } if text == "still here"));
    assert!(matches!(&c.rows[&2], Row::UserInput { text, .. } if text == "hi"));
    assert!(c.queue_edit_allowed);
    assert_eq!(c.queue.as_ref().map(|q| q.items.len()), Some(1));
    assert_eq!(c.pending_interactions.len(), 1);
    assert_eq!(
        c.pending_interactions[0].kind_class(),
        InteractionKind::Unsupported
    );
    let running = &c.subagents.as_ref().unwrap().running;
    assert_eq!(running[0].stop_control.as_ref().unwrap().task_id, "task");
}

#[test]
fn deltas_with_new_markers_and_ops_apply_the_rest_of_the_batch() {
    let mut c = ConversationState::default();
    c.apply_snapshot(&v3_18_2_snapshot());
    c.apply_deltas(&[
        json!({ "op": "row.appended", "row": {
            "rowId": 7, "turnId": "t2", "kind": "timelineMarker", "createdAt": 2, "createdAtSeq": 7,
            "marker": { "type": "worktreeCreated", "path": "C:/wt2", "sha": "def",
                        "ref": "main", "output": "" } } }),
        json!({ "op": "someFutureOp", "payload": { "x": 1 } }),
        json!({ "op": "state.updated", "patch": {
            "availability": { "queueEdit": { "allowed": false },
                              "rewindFiles": { "allowed": true } },
            "pendingInteractions": [] } }),
        json!({ "op": "row.appended", "row": {
            "rowId": 8, "turnId": "t2", "kind": "assistantText", "text": "after",
            "state": "streaming", "createdAt": 2, "createdAtSeq": 8 } }),
    ]);
    assert!(matches!(c.rows[&7], Row::Other));
    assert!(matches!(&c.rows[&8], Row::AssistantText { text, .. } if text == "after"));
    assert!(!c.queue_edit_allowed);
    assert!(c.pending_interactions.is_empty());
}

#[test]
fn older_rows_page_with_unknown_kinds_still_merges() {
    let mut c = ConversationState::default();
    c.apply_snapshot(&v3_18_2_snapshot());
    c.apply_older_rows(
        &[
            json!({ "rowId": 0, "turnId": "t0", "kind": "unreadable", "sourceKind": "x",
                    "createdAt": 0, "createdAtSeq": 0 }),
        ],
        Some(false),
    );
    assert_eq!(c.rows.len(), 7);
    assert!(!c.has_more_history());
}
