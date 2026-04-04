// ============================================================================
// Event Projection — SessionEvent → EventFrame
//
// Pure translation logic: maps internal typed events to the external proto
// transport format. No I/O, no state, no side effects.
//
// Adding a new SessionEvent variant requires a corresponding case here and
// in the proto `EventFrame` consumer. The exhaustive match enforces this.
// ============================================================================

use chrono::Utc;
use serde_json::{Value, json};

use hahi_proto::events::EventFrame;

use crate::infra::events::SessionEvent;

/// Translate a `SessionEvent` to the `EventFrame` proto for the gRPC stream.
///
/// `RunCompleted` must be projected via [`hub_run_completed_to_frame`] — it
/// carries completion data that requires a separate async registry lookup.
/// Passing `RunCompleted` here is a logic error and will panic.
pub fn hub_event_to_frame(
    cursor: String,
    run_id: String,
    thread_id: String,
    event: &SessionEvent,
) -> EventFrame {
    let (event_type, data_json) = match event {
        SessionEvent::RunStarted => ("run.started", json!({}).to_string()),
        SessionEvent::TextDelta { text } => {
            ("output.text_delta", json!({ "text": text }).to_string())
        }
        SessionEvent::ThinkingDelta { text } => {
            ("output.thinking_delta", json!({ "text": text }).to_string())
        }
        SessionEvent::ToolStart { id, name, input_preview } => (
            "tool.started",
            json!({ "id": id, "name": name, "input_preview": input_preview }).to_string(),
        ),
        SessionEvent::ToolResult { id, name, content, is_error } => (
            "tool.result",
            json!({ "id": id, "name": name, "content": content, "is_error": is_error }).to_string(),
        ),
        SessionEvent::RunCompleted { .. } => {
            unreachable!("RunCompleted must be projected via hub_run_completed_to_frame")
        }
        SessionEvent::RunFailed { reason } => {
            ("run.failed", json!({ "reason": reason }).to_string())
        }
        SessionEvent::ControlRequested { request_id, kind, payload_json } => (
            "control.requested",
            json!({
                "request_id": request_id,
                "kind": kind,
                "payload": parse_payload_json(payload_json),
            })
            .to_string(),
        ),
        SessionEvent::Compacted { pre_tokens } => (
            "context.compacted",
            json!({ "pre_tokens": pre_tokens }).to_string(),
        ),
    };

    EventFrame {
        cursor,
        run_id,
        thread_id,
        emitted_at: Utc::now().to_rfc3339(),
        r#type: event_type.to_string(),
        data_json,
    }
}

/// Project a `RunCompleted` event into an `EventFrame`, attaching the
/// persisted message content from the completion channel.
pub fn hub_run_completed_to_frame(
    cursor: String,
    run_id: String,
    thread_id: String,
    message_id: String,
    content: String,
    input_tokens: u32,
    output_tokens: u32,
) -> EventFrame {
    EventFrame {
        cursor,
        run_id,
        thread_id,
        emitted_at: Utc::now().to_rfc3339(),
        r#type: "run.completed".to_string(),
        data_json: json!({
            "message_id": message_id,
            "content": content,
            "usage": {
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "total_tokens": input_tokens + output_tokens,
            }
        })
        .to_string(),
    }
}

pub fn parse_payload_json(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| json!({ "raw": raw }))
}
