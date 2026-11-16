//! Conversation transcript rows: turnHeader, userInput, assistantText, reasoning, toolCall.
//!
//! Spec source: packages/shared/src/zcode-protocol-v4/ (rows.ts:1-240).

use serde_json::Value;

/// Write/Edit refuse an existing file the agent hasn't read (or that changed
/// since). The wire `error.code` is only the generic `ToolExecutionFailed`, so
/// the row is recognized by the CLI's fixed messages (core
/// `tool/handlers/write.ts` and `edit.ts`, `*_NOT_READ_MESSAGE` /
/// `*_STALE_MESSAGE`).
pub fn is_needs_read_error(message: &str) -> bool {
    message.contains("File has not been read yet.")
        || message.contains("File has been modified since read,")
}

#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    TurnHeader {
        row_id: u64,
        entity_id: String,
        state: String,
        active_ms: Option<u64>,
        file_changes: Option<crate::conversation::turn_meta::FileChanges>,
        can_rewind: bool,
    },
    UserInput {
        row_id: u64,
        entity_id: String,
        text: String,
        can_edit: bool,
        /// realUser | backgroundResult | goalContinuation | mailbox | synthetic | workflowLaunch
        origin: String,
        /// `createdAt`, Unix epoch milliseconds.
        created_at: Option<u64>,
        attachments: Vec<crate::composer::attachment::AttachmentRef>,
    },
    AssistantText {
        row_id: u64,
        entity_id: String,
        text: String,
        state: String,
        can_retry: bool,
        created_at: Option<u64>,
    },
    Reasoning {
        row_id: u64,
        text: String,
        duration_ms: Option<u64>,
    },
    ToolCall {
        row_id: u64,
        label: String,
        /// Wire `toolName`; renderers dispatch on this, `label` is display text.
        tool_name: String,
        status: String,
        input_text: String,
        output_text: String,
        tool_call_id: Option<String>,
        /// `display` of the row, else of `output` (toolDisplay.ts payloads).
        display: Option<Value>,
        /// An error row that is Write/Edit's "read the file first" guard, not
        /// a real failure (see [`is_needs_read_error`]).
        needs_read: bool,
    },
    Subagent {
        row_id: u64,
        subagent_type: String,
        status: String,
        summary_text: String,
        parent_tool_call_id: Option<String>,
        child_session_id: Option<String>,
        work_id: Option<String>,
        backgrounded: bool,
        started_at: Option<u64>,
    },
    Other,
}

impl Row {
    pub fn from_value(v: &Value) -> Row {
        let str_field = |name: &str| {
            v.get(name)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        let row_id = v.get("rowId").and_then(Value::as_u64).unwrap_or(0);
        let created_at = v
            .get("createdAt")
            .and_then(Value::as_f64)
            .filter(|t| *t > 0.)
            .map(|t| t as u64);
        let entity_id = v
            .get("entityId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let actions = v.get("actions");
        let can_edit = actions
            .and_then(|a| a.get("canEdit"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let can_retry = actions
            .and_then(|a| a.get("canRetry"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let can_rewind = actions
            .and_then(|a| a.get("canRewindFiles"))
            .and_then(Value::as_bool)
            .unwrap_or(false);

        match str_field("kind").as_str() {
            "turnHeader" => {
                let active_ms = v.get("activeMs").and_then(Value::as_u64);
                let file_changes = v
                    .get("fileChanges")
                    .and_then(crate::conversation::turn_meta::FileChanges::from_value);
                Row::TurnHeader {
                    row_id,
                    entity_id,
                    state: str_field("state"),
                    active_ms,
                    file_changes,
                    can_rewind,
                }
            }
            "userInput" => Row::UserInput {
                row_id,
                entity_id,
                text: str_field("text"),
                can_edit,
                origin: str_field("origin"),
                created_at,
                attachments: v
                    .get("attachments")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|r| serde_json::from_value(r.clone()).ok())
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            "assistantText" => Row::AssistantText {
                row_id,
                entity_id,
                text: str_field("text"),
                state: str_field("state"),
                can_retry,
                created_at,
            },
            "reasoning" => {
                let duration_ms = v.get("durationMs").and_then(Value::as_u64);
                Row::Reasoning {
                    row_id,
                    text: str_field("text"),
                    duration_ms,
                }
            }
            "toolCall" => {
                let input_text = v
                    .get("inputText")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let output_text = v
                    .get("output")
                    .and_then(|o| o.get("text").or_else(|| o.get("preview")))
                    .and_then(Value::as_str)
                    .or_else(|| {
                        v.get("outputPreview")
                            .and_then(|p| p.get("text"))
                            .and_then(Value::as_str)
                    })
                    .unwrap_or("")
                    .to_string();

                let tool_call_id = v
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);

                let display = v
                    .get("display")
                    .or_else(|| v.get("output").and_then(|o| o.get("display")))
                    .filter(|d| d.is_object())
                    .cloned();
                let status = str_field("status");
                let needs_read = status == "error"
                    && matches!(str_field("toolName").as_str(), "Write" | "Edit")
                    && v.get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(Value::as_str)
                        .is_some_and(is_needs_read_error);
                Row::ToolCall {
                    row_id,
                    label: ["label", "title", "toolName", "name"]
                        .iter()
                        .find_map(|k| v.get(*k).and_then(Value::as_str))
                        .unwrap_or("tool")
                        .to_string(),
                    tool_name: str_field("toolName"),
                    status,
                    input_text,
                    output_text,
                    tool_call_id,
                    display,
                    needs_read,
                }
            }
            "subagent" => {
                let parent_tool_call_id = v
                    .get("parentToolCallId")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                let subagent_type = str_field("subagentType");
                let status = str_field("status");
                let summary_text = str_field("summaryText");
                let child_session_id = v
                    .get("childSessionId")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                let work_id = v
                    .get("workId")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                let backgrounded = v
                    .get("backgrounded")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let started_at = v.get("startedAt").and_then(Value::as_u64);

                Row::Subagent {
                    row_id,
                    subagent_type,
                    status,
                    summary_text,
                    parent_tool_call_id,
                    child_session_id,
                    work_id,
                    backgrounded,
                    started_at,
                }
            }
            _ => Row::Other,
        }
    }

    pub fn stream_field_mut(&mut self, path: &str) -> Option<&mut String> {
        match (self, path) {
            (
                Row::UserInput { text, .. }
                | Row::AssistantText { text, .. }
                | Row::Reasoning { text, .. },
                "text",
            ) => Some(text),
            (Row::Subagent { summary_text, .. }, "summaryText") => Some(summary_text),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn text_mut(&mut self) -> Option<&mut String> {
        self.stream_field_mut("text")
    }
}
