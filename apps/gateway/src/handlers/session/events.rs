// ============================================================================
// Server-Sent Events Handler
//
// GET /v1/runs/:run_id/events
//
// Calls session service StreamEvents gRPC and forwards each EventFrame to the
// client as a single SSE event with stable semantics in the frame payload.
//
// Reconnect protocol:
//   Client sends "Last-Event-ID: <event_id>" header on reconnect.
//   We forward this as last_event_id to the session gRPC call, which
//   replays missed events from Redis before streaming live ones.
//
// Each SSE frame:
//   id: <cursor>
//   event: execution
//   data: <EventFrame JSON>
//
// Heartbeat:
//   Every 15 seconds of silence, a `heartbeat` event is sent so clients
//   and intermediaries (CDNs, mobile networks) don't drop the connection
//   during long-running or thinking-heavy agent turns.
//
// Connection ends when the gRPC stream closes (run completed or failed).
// ============================================================================

use std::time::Duration;

use axum::response::sse::KeepAliveStream;
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::{
        Sse,
        sse::{Event, KeepAlive},
    },
};
use tokio::time::{MissedTickBehavior, interval};
use tokio_stream::StreamExt;

use crate::config::AppState;
use crate::error::GatewayError;

type SseInner = std::pin::Pin<
    Box<dyn futures::Stream<Item = std::result::Result<Event, std::convert::Infallible>> + Send>,
>;

/// GET /v1/runs/:run_id/events
pub async fn stream_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
) -> std::result::Result<Sse<KeepAliveStream<SseInner>>, GatewayError> {
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let mut grpc_stream: tonic::codec::Streaming<_> =
        state.session.stream_events(run_id, last_event_id).await?;

    let event_stream: SseInner = Box::pin(async_stream::stream! {
        let mut ticker = interval(Duration::from_secs(15));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        // Skip the first immediate tick — we don't want a heartbeat before the
        // first real event.
        ticker.tick().await;

        loop {
            tokio::select! {
                // Real gRPC frames take priority over the heartbeat ticker.
                biased;

                result = grpc_stream.next() => {
                    match result {
                        None => return,
                        Some(Err(e)) => {
                            tracing::warn!(error = %e, "gRPC stream error");
                            yield Ok(Event::default()
                                .event("error")
                                .data(serde_json::json!({ "reason": e.message() }).to_string()));
                            return;
                        }
                        Some(Ok(session_event)) => {
                            let event_id = session_event.cursor.clone();
                            let data = serde_json::json!({
                                "cursor": session_event.cursor,
                                "run_id": session_event.run_id,
                                "thread_id": session_event.thread_id,
                                "emitted_at": session_event.emitted_at,
                                "type": session_event.r#type,
                                "data_json": session_event.data_json,
                            })
                            .to_string();

                            yield Ok(Event::default()
                                .id(event_id)
                                .event("execution")
                                .data(data));
                        }
                    }
                }

                _ = ticker.tick() => {
                    yield Ok(Event::default()
                        .event("heartbeat")
                        .data(r#"{"type":"heartbeat"}"#));
                }
            }
        }
    });

    Ok(Sse::new(event_stream).keep_alive(KeepAlive::default()))
}
