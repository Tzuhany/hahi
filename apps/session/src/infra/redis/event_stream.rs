// ============================================================================
// Redis Agent Event Stream
//
// Reads agent events from Redis Stream `results:{run_id}` and
// translates them into typed SessionEvent values.
//
// One subscriber task runs per active Run. It blocks on XREAD, parses each
// entry, and sends events through a channel consumed by the application layer.
//
// Reconnect semantics:
//   Pass `last_event_id` to resume from a specific Redis Stream offset.
//   Empty string means start from the beginning of the stream.
// ============================================================================

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;

use crate::domain::RunId;
use crate::infra::events::SessionEvent;
use crate::ports::event_stream::AgentEventStream;

/// How long XREAD blocks waiting for new entries (milliseconds).
/// 5 seconds keeps connections alive without busy-polling.
const BLOCK_MS: usize = 5_000;

/// Maximum entries fetched per XREAD call.
/// 100 keeps individual responses small while allowing burst catch-up.
const READ_COUNT: usize = 100;

pub struct RedisEventStream {
    client: redis::Client,
}

impl RedisEventStream {
    pub fn new(client: redis::Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl AgentEventStream for RedisEventStream {
    async fn subscribe(
        &self,
        run_id: &RunId,
        last_event_id: &str,
    ) -> Result<tokio::sync::mpsc::Receiver<SessionEvent>> {
        let stream_key = format!("results:{}", run_id.as_str());
        let start_id = if last_event_id.is_empty() {
            "0".to_string()
        } else {
            last_event_id.to_string()
        };

        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .context("failed to connect to Redis")?;

        let (tx, rx) = tokio::sync::mpsc::channel(512);
        let run_id = run_id.clone();

        tokio::spawn(async move {
            let mut last_id = start_id;

            loop {
                let result: redis::RedisResult<redis::streams::StreamReadReply> =
                    redis::cmd("XREAD")
                        .arg("BLOCK")
                        .arg(BLOCK_MS)
                        .arg("COUNT")
                        .arg(READ_COUNT)
                        .arg("STREAMS")
                        .arg(&stream_key)
                        .arg(&last_id)
                        .query_async(&mut conn)
                        .await;

                match result {
                    Err(e) => {
                        tracing::warn!(run_id = %run_id, error = %e, "Redis XREAD failed");
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    }
                    Ok(reply) => {
                        for stream in reply.keys {
                            for entry in stream.ids {
                                last_id = entry.id.clone();

                                match parse_entry(&entry) {
                                    Some(event) => {
                                        // Channel closed = downstream dropped the receiver (run finished or cancelled).
                                        if tx.send(event).await.is_err() {
                                            return;
                                        }
                                    }
                                    None => {
                                        tracing::trace!(
                                            id = %entry.id,
                                            "skipped unrecognized Redis Stream entry"
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok(rx)
    }
}

/// Parse a Redis Stream entry into a `SessionEvent`.
///
/// Supports two formats:
///   - v2 (current): `type = "agent_event"`, `data = {"v":2, "event": "<base64 prost bytes>"}`
///   - v1 (legacy):  `type = "<event_type>"`, `data = <JSON payload>` (or flat fields)
///
/// The legacy path is kept for rolling deploys where old agent pods may still
/// be writing JSON entries while session has already been updated.
fn parse_entry(entry: &redis::streams::StreamId) -> Option<SessionEvent> {
    let envelope = EventEnvelope::from_entry(entry)?;

    if envelope.event_type == "agent_event" {
        return parse_proto_entry(&envelope.data);
    }

    // Legacy JSON path — kept for rolling-deploy compatibility.
    parse_legacy_entry(&envelope)
}

/// Decode a v2 proto entry: base64 → prost bytes → `AgentStreamEvent` → `SessionEvent`.
fn parse_proto_entry(data: &Value) -> Option<SessionEvent> {
    use base64::Engine as _;
    use hahi_proto::agent_event::{AgentStreamEvent, agent_stream_event::Event};
    use tonic_prost::prost::Message as _;

    let encoded = data.get("event")?.as_str()?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).ok()?;
    let msg = AgentStreamEvent::decode(&bytes[..]).ok()?;

    match msg.event? {
        Event::SessionStateChanged(e) => {
            if e.state == "running" {
                Some(SessionEvent::RunStarted)
            } else {
                None
            }
        }
        Event::TextDelta(e) => Some(SessionEvent::TextDelta { text: e.text }),
        Event::ThinkingDelta(e) => Some(SessionEvent::ThinkingDelta { text: e.text }),
        Event::ToolStart(e) => Some(SessionEvent::ToolStart {
            id: e.id,
            name: e.name,
            input_preview: e.input_preview,
        }),
        Event::ToolResult(e) => Some(SessionEvent::ToolResult {
            id: e.id,
            name: e.name,
            content: e.content,
            is_error: e.is_error,
        }),
        Event::TurnEnd(e) => {
            let stop = &e.stop_reason;
            // Use starts_with — stop_reason may carry large payloads (e.g. PlanReview
            // embeds the full plan text which can contain "Error" as a word).
            if stop == "Completed" || stop.starts_with("Completed") {
                Some(SessionEvent::RunCompleted {
                    input_tokens: e.input_tokens as u32,
                    output_tokens: e.output_tokens as u32,
                })
            } else if stop.starts_with("Error(") {
                Some(SessionEvent::RunFailed { reason: stop.clone() })
            } else {
                // RequiresAction, PlanReview, DiminishingReturns etc. — run stays
                // alive; control_request event follows.
                None
            }
        }
        Event::ControlRequest(e) => Some(SessionEvent::ControlRequested {
            request_id: e.request_id,
            kind: e.kind,
            payload_json: e.payload_json,
        }),
        Event::Compacted(e) => Some(SessionEvent::Compacted { pre_tokens: e.pre_tokens }),
        // Collapsed, HookBlocked, PlanModeChanged are informational — no current
        // SessionEvent mapping; ignored until the app layer needs them.
        Event::Collapsed(_) | Event::HookBlocked(_) | Event::PlanModeChanged(_) => None,
        Event::RunFailed(e) => Some(SessionEvent::RunFailed { reason: e.reason }),
    }
}

/// Parse a v1 (legacy JSON) entry.
fn parse_legacy_entry(envelope: &EventEnvelope) -> Option<SessionEvent> {
    match envelope.event_type.as_str() {
        "session_state_changed" => {
            let state = envelope.data.get("state")?.as_str()?;
            if state == "running" { Some(SessionEvent::RunStarted) } else { None }
        }
        "stream" => {
            let payload = &envelope.data;
            match payload.get("type")?.as_str()? {
                "text_delta" => Some(SessionEvent::TextDelta {
                    text: payload.get("text")?.as_str()?.to_string(),
                }),
                "thinking_delta" => Some(SessionEvent::ThinkingDelta {
                    text: payload.get("text")?.as_str()?.to_string(),
                }),
                "tool_use_start" => Some(SessionEvent::ToolStart {
                    id: payload.get("id")?.as_str()?.to_string(),
                    name: payload.get("name")?.as_str()?.to_string(),
                    input_preview: String::new(),
                }),
                _ => None,
            }
        }
        "tool_start" => Some(SessionEvent::ToolStart {
            id: envelope.data.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
            name: envelope.data.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
            input_preview: envelope.data.get("input_preview").and_then(Value::as_str).unwrap_or_default().to_string(),
        }),
        "tool_result" => Some(SessionEvent::ToolResult {
            id: envelope.data.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
            name: envelope.data.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
            content: envelope.data.get("content").and_then(Value::as_str).unwrap_or_default().to_string(),
            is_error: envelope.data.get("is_error").and_then(Value::as_bool).unwrap_or(false),
        }),
        "turn_end" => {
            let stop_reason = envelope.data.get("stop_reason").and_then(|v| v.as_str()).unwrap_or("Completed");
            if stop_reason == "Completed" || stop_reason.starts_with("Completed") {
                Some(SessionEvent::RunCompleted {
                    input_tokens: envelope.data.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    output_tokens: envelope.data.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                })
            } else if stop_reason.starts_with("Error(") {
                Some(SessionEvent::RunFailed { reason: stop_reason.to_string() })
            } else {
                None
            }
        }
        "control_request" => Some(SessionEvent::ControlRequested {
            request_id: envelope.data.get("request_id").and_then(Value::as_str).unwrap_or_default().to_string(),
            kind: envelope.data.get("kind").or_else(|| envelope.data.get("type")).and_then(Value::as_str).unwrap_or_default().to_string(),
            payload_json: envelope.data.get("payload").map(Value::to_string).unwrap_or_else(|| "{}".to_string()),
        }),
        "compacted" => Some(SessionEvent::Compacted {
            pre_tokens: envelope.data.get("pre_tokens").and_then(Value::as_u64).unwrap_or(0),
        }),
        _ => None,
    }
}

/// A decoded Redis Stream entry before semantic dispatch.
///
/// Agent writes entries in one of two formats:
///   - Current: `type` field + `data` field (JSON-serialized payload)
///   - Legacy:  `type` field + individual flat fields (pre-envelope era)
///
/// `from_entry` tries the current format first, then falls back to `legacy_data`.
struct EventEnvelope {
    event_type: String,
    data: Value,
}

impl EventEnvelope {
    fn from_entry(entry: &redis::streams::StreamId) -> Option<Self> {
        let get = |key: &str| -> Option<String> {
            entry.map.get(key).and_then(|v| match v {
                redis::Value::BulkString(b) => String::from_utf8(b.clone()).ok(),
                _ => None,
            })
        };

        let event_type = get("type")?;
        let data = get("data")
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .or_else(|| legacy_data(entry, &event_type))?;

        Some(Self { event_type, data })
    }
}

/// Reconstruct a JSON data payload from legacy flat-field Redis entries.
///
/// The agent previously wrote individual fields directly into the Redis Stream
/// entry instead of a single serialized `data` field. This function handles
/// those older entries so the system remains compatible during rolling deploys.
fn legacy_data(entry: &redis::streams::StreamId, event_type: &str) -> Option<Value> {
    let get = |key: &str| -> Option<String> {
        entry.map.get(key).and_then(|v| match v {
            redis::Value::BulkString(b) => String::from_utf8(b.clone()).ok(),
            _ => None,
        })
    };

    match event_type {
        "session_state_changed" => Some(serde_json::json!({
            "state": get("state")?,
        })),
        "stream" | "turn_end" => get("payload").and_then(|raw| serde_json::from_str(&raw).ok()),
        "tool_start" => Some(serde_json::json!({
            "id": get("id").unwrap_or_default(),
            "name": get("name").unwrap_or_default(),
            "input_preview": get("input_preview").unwrap_or_default(),
        })),
        "tool_result" => Some(serde_json::json!({
            "id": get("id").unwrap_or_default(),
            "name": get("name").unwrap_or_default(),
            "content": get("content").unwrap_or_default(),
            "is_error": get("is_error").map(|v| v == "true").unwrap_or(false),
        })),
        "control_request" => Some(serde_json::json!({
            "request_id": get("request_id").unwrap_or_default(),
            "kind": get("kind").or_else(|| get("type")).unwrap_or_default(),
            "payload": get("payload")
                .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
                .unwrap_or_else(|| serde_json::json!({})),
        })),
        "compacted" => Some(serde_json::json!({
            "pre_tokens": get("pre_tokens").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0),
        })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn entry(fields: &[(&str, &str)]) -> redis::streams::StreamId {
        let map = fields
            .iter()
            .map(|(k, v)| {
                (
                    (*k).to_string(),
                    redis::Value::BulkString((*v).as_bytes().to_vec()),
                )
            })
            .collect::<HashMap<_, _>>();
        redis::streams::StreamId {
            id: "1-0".to_string(),
            map,
        }
    }

    #[test]
    fn parses_current_data_envelope() {
        let parsed = parse_entry(&entry(&[
            ("type", "tool_start"),
            (
                "data",
                r#"{"id":"t1","name":"WebSearch","input_preview":"cats"}"#,
            ),
        ]))
        .expect("event should parse");

        match parsed {
            SessionEvent::ToolStart {
                id,
                name,
                input_preview,
            } => {
                assert_eq!(id, "t1");
                assert_eq!(name, "WebSearch");
                assert_eq!(input_preview, "cats");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn parses_legacy_flat_fields() {
        let parsed = parse_entry(&entry(&[
            ("type", "tool_result"),
            ("id", "t2"),
            ("name", "WebFetch"),
            ("content", "done"),
            ("is_error", "false"),
        ]))
        .expect("event should parse");

        match parsed {
            SessionEvent::ToolResult {
                id,
                name,
                content,
                is_error,
            } => {
                assert_eq!(id, "t2");
                assert_eq!(name, "WebFetch");
                assert_eq!(content, "done");
                assert!(!is_error);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
