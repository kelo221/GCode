use super::*;
use crate::conversation::model::Row;
use crate::transcript::noise_free::{Place, Summary, kind, place};
use serde_json::json;

fn edit_display(path: &str, errors: u32, extra: Value) -> Value {
    let mut diagnostics = json!({
        "errors": errors,
        "warnings": 0,
        "availability": "ok",
        "freshness": "confirmed",
        "check": "not_requested"
    });
    for (key, value) in extra.as_object().into_iter().flatten() {
        diagnostics[key] = value.clone();
    }
    json!({
        "kind": "file_diff",
        "filePath": path,
        "additions": 1,
        "deletions": 1,
        "structuredPatch": [],
        "diagnostics": diagnostics
    })
}

fn edit(row_id: u64, display: Value) -> Row {
    Row::ToolCall {
        row_id,
        label: "Edit".into(),
        tool_name: "Edit".into(),
        status: "success".into(),
        input_text: "{}".into(),
        output_text: String::new(),
        tool_call_id: None,
        display: Some(display),
        needs_read: false,
    }
}

#[test]
fn chip_marks_unconfirmed_results_and_pending_servers() {
    let parse = |display: Value| chip_text(edit_diagnostics(Some(&display)).unwrap());
    assert_eq!(parse(edit_display("/a.rs", 2, json!({}))), "2 errors");
    assert_eq!(parse(edit_display("/a.rs", 1, json!({}))), "1 error");
    assert_eq!(parse(edit_display("/a.rs", 0, json!({}))), "No problems");
    assert_eq!(
        parse(edit_display(
            "/a.ts",
            0,
            json!({ "freshness": "best_effort" })
        )),
        "No problems?"
    );
    assert_eq!(
        parse(edit_display(
            "/a.rs",
            0,
            json!({ "warnings": 3, "timedOut": true })
        )),
        "3 warnings?"
    );
    assert_eq!(
        parse(edit_display(
            "/a.rs",
            0,
            json!({ "availability": "starting" })
        )),
        "Diagnostics pending"
    );
}

#[test]
fn displays_without_a_summary_have_no_chip() {
    let plain = json!({ "kind": "file_diff", "filePath": "/a.rs", "additions": 0, "deletions": 0, "structuredPatch": [] });
    assert!(edit_diagnostics(Some(&plain)).is_none());
    assert!(edit_diagnostics(Some(&json!({ "kind": "task_output" }))).is_none());
    assert!(edit_diagnostics(None).is_none());
}

#[test]
fn fold_counts_the_latest_result_per_file() {
    let rows = [
        edit(1, edit_display(r"C:\work\a.rs", 2, json!({}))),
        edit(2, edit_display(r"C:\work\b.rs", 1, json!({}))),
        // The same file, written with other separators and case, now fixed.
        edit(3, edit_display("c:/work/A.rs", 0, json!({}))),
        edit(
            4,
            edit_display(r"C:\work\c.rs", 5, json!({ "availability": "starting" })),
        ),
    ];
    assert_eq!(
        place(0, rows.len(), |i| kind(&rows[i])),
        Place::Leader(Summary {
            count: 4,
            running: 0,
            failed: 0,
            errors: 1
        })
    );
}
