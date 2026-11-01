//! Tests for `conversation/auto_compact.rs`: idle tracking, eligibility and
//! the in-flight steps.

use super::*;
use crate::conversation::context_usage::ContextWindow;
use serde_json::json;

const ON: AutoCompactEntry = AutoCompactEntry {
    enabled: true,
    minutes: 50,
};

fn window(used: f64) -> Option<ContextWindow> {
    Some(ContextWindow {
        used,
        max: 200_000.,
        cache_hit_rate: None,
        segments: Vec::new(),
    })
}

/// Idle since `t0` with a big context.
fn idle(t0: Instant) -> ConversationState {
    let mut c = ConversationState {
        context_window: window(80_000.),
        ..Default::default()
    };
    c.note_control(false, t0);
    c
}

fn later(t0: Instant, minutes: u64) -> Instant {
    t0 + Duration::from_secs(minutes * 60)
}

#[test]
fn leaving_the_active_phase_starts_the_idle_clock_once() {
    let t0 = Instant::now();
    let mut c = ConversationState::default();
    c.note_control(true, t0);
    assert_eq!(c.idle_since, None);
    c.note_control(false, t0);
    assert_eq!(c.idle_since, Some(t0));
    // Later idle updates keep the first idle instant.
    c.note_control(false, later(t0, 1));
    assert_eq!(c.idle_since, Some(t0));
}

#[test]
fn a_new_run_clears_idle_and_the_compacted_flag() {
    let t0 = Instant::now();
    let mut c = idle(t0);
    c.auto_compacted = true;
    c.note_control(true, later(t0, 1));
    assert_eq!(c.idle_since, None);
    assert!(!c.auto_compacted);
}

#[test]
fn control_frames_through_apply_control_track_the_phase() {
    let mut c = ConversationState::default();
    c.apply_control(&json!({ "phase": "running" }));
    assert_eq!(c.idle_since, None);
    c.apply_control(&json!({ "phase": "idle" }));
    assert!(c.idle_since.is_some());
}

#[test]
fn due_after_the_configured_minutes_only() {
    let t0 = Instant::now();
    let c = idle(t0);
    assert!(!c.auto_compact_due(ON, later(t0, 49)));
    assert!(c.auto_compact_due(ON, later(t0, 50)));
}

#[test]
fn not_due_when_disabled() {
    let t0 = Instant::now();
    let off = AutoCompactEntry {
        enabled: false,
        ..ON
    };
    assert!(!idle(t0).auto_compact_due(off, later(t0, 500)));
}

#[test]
fn not_due_below_the_token_floor_or_without_usage() {
    let t0 = Instant::now();
    let mut c = idle(t0);
    c.context_window = window(TOKEN_FLOOR - 1.);
    assert!(!c.auto_compact_due(ON, later(t0, 60)));
    c.context_window = None;
    assert!(!c.auto_compact_due(ON, later(t0, 60)));
}

#[test]
fn not_due_twice_in_one_idle_stretch() {
    let t0 = Instant::now();
    let mut c = idle(t0);
    c.begin_auto_compact();
    c.auto_compact = None; // finished
    assert!(!c.auto_compact_due(ON, later(t0, 120)));
    // New activity re-arms it.
    c.note_control(true, later(t0, 121));
    c.note_control(false, later(t0, 122));
    assert!(c.auto_compact_due(ON, later(t0, 172)));
}

#[test]
fn not_due_while_running_or_with_live_work() {
    let t0 = Instant::now();
    let mut c = idle(t0);
    c.phase = "running".into();
    c.note_control(true, later(t0, 60));
    assert!(!c.auto_compact_due(ON, later(t0, 60)));

    let mut c = idle(t0);
    c.queue = crate::conversation::queue::QueueState::from_value(&json!({
        "items": [{ "queueItemId": "q1", "text": "next" }]
    }));
    assert!(c.queue.as_ref().is_some_and(|q| !q.items.is_empty()));
    assert!(!c.auto_compact_due(ON, later(t0, 60)));
}

fn sent(id: &str) -> Option<AutoCompactFlight> {
    Some(AutoCompactFlight::Sent {
        command_id: id.into(),
    })
}

fn running(id: &str) -> Option<AutoCompactFlight> {
    Some(AutoCompactFlight::Running {
        command_id: id.into(),
    })
}

fn marker(command_id: &str, status: &str) -> serde_json::Value {
    json!({
        "rowId": 9,
        "kind": "timelineMarker",
        "sourceCommandId": command_id,
        "marker": { "type": "compact", "origin": "manual", "status": status },
    })
}

#[test]
fn only_our_terminal_marker_ends_the_flight() {
    let t0 = Instant::now();
    let mut c = idle(t0);
    c.begin_auto_compact();
    c.auto_compact = running("cmd-1");
    c.note_compact_marker(&marker("cmd-1", "running"));
    assert_eq!(c.auto_compact, running("cmd-1"), "running is not terminal");
    c.note_compact_marker(&marker("someone-else", "success"));
    assert_eq!(c.auto_compact, running("cmd-1"), "another command's marker");
    c.note_compact_marker(&marker("cmd-1", "noop"));
    assert_eq!(c.auto_compact, None);
    assert_eq!(c.auto_compact_outcome.as_deref(), Some("noop"));
    assert!(c.auto_compacted);
    assert!(!c.auto_compact_due(ON, later(t0, 200)));
}

#[test]
fn a_marker_before_the_ack_ends_the_flight_and_the_late_ack_is_ignored() {
    let t0 = Instant::now();
    let mut c = idle(t0);
    c.begin_auto_compact();
    c.auto_compact = sent("cmd-2");
    c.apply_deltas(&[json!({ "op": "row.appended", "row": marker("cmd-2", "failed") })]);
    assert_eq!(c.auto_compact, None);
    assert_eq!(c.auto_compact_outcome.as_deref(), Some("failed"));
    c.note_auto_compact_ack(true);
    assert_eq!(c.auto_compact, None);
}

#[test]
fn a_resync_snapshot_carrying_the_marker_ends_the_flight() {
    let t0 = Instant::now();
    let mut c = idle(t0);
    c.begin_auto_compact();
    c.auto_compact = running("cmd-3");
    c.apply_snapshot(&json!({ "rows": { "window": [marker("cmd-3", "success")] } }));
    assert_eq!(c.auto_compact, None);
    assert_eq!(c.auto_compact_outcome.as_deref(), Some("success"));
}

#[test]
fn ack_moves_sent_to_running_and_rejection_ends_the_flight() {
    let t0 = Instant::now();
    let mut c = idle(t0);
    c.begin_auto_compact();
    c.auto_compact = sent("cmd-4");
    c.note_auto_compact_ack(true);
    assert_eq!(c.auto_compact, running("cmd-4"));
    c.auto_compact = sent("cmd-4");
    c.note_auto_compact_ack(false);
    assert_eq!(c.auto_compact, None);
    assert!(c.auto_compacted, "no retry until new activity");
}

#[test]
fn the_compactions_own_trailing_running_frame_does_not_rearm_it() {
    let t0 = Instant::now();
    let mut c = idle(t0);
    c.begin_auto_compact();
    c.auto_compact = running("cmd-5");
    c.apply_control(&json!({ "phase": "running" }));
    c.note_compact_marker(&marker("cmd-5", "success"));
    assert_eq!(c.auto_compact, Some(AutoCompactFlight::Settling));
    // Still the compaction's turn, not new activity.
    c.apply_control(&json!({ "phase": "running" }));
    assert!(c.auto_compacted);
    c.apply_control(&json!({ "phase": "idle" }));
    assert_eq!(c.auto_compact, None);
    assert!(!c.auto_compact_due(ON, later(t0, 600)));
}

#[test]
fn not_due_again_until_rows_are_appended() {
    let t0 = Instant::now();
    let mut c = idle(t0);
    c.begin_auto_compact();
    c.auto_compact = running("cmd-6");
    c.apply_deltas(&[json!({ "op": "row.appended", "row": marker("cmd-6", "success") })]);
    assert_eq!(c.compacted_through, Some(9));
    // Activity that adds no rows (e.g. another compaction) doesn't count.
    c.note_control(true, later(t0, 1));
    c.note_control(false, later(t0, 2));
    assert!(!c.auto_compact_due(ON, later(t0, 120)));
    c.apply_deltas(&[json!({
        "op": "row.appended",
        "row": { "rowId": 10, "kind": "userInput", "text": "next" },
    })]);
    assert!(c.auto_compact_due(ON, later(t0, 120)));
}

#[test]
fn a_run_starting_while_resuming_cancels_the_compaction() {
    let t0 = Instant::now();
    let mut c = idle(t0);
    c.begin_auto_compact();
    c.note_control(true, later(t0, 51));
    assert_eq!(c.auto_compact, None);
    assert!(!c.auto_compacted);
}
