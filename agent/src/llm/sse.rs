// ============================================================================
// Generic SSE Stream Infrastructure
//
// Both Anthropic and OpenAI deliver streaming responses as Server-Sent Events
// over HTTP. The byte-level mechanics are identical; only the event semantics
// differ. This module owns the generic machinery:
//
//   - Byte buffering (HTTP chunked transfer → complete SSE frames)
//   - Frame extraction (event: <type>\ndata: <json>\n\n)
//   - Yielding StreamEvents via the SseEventParser trait
//
// Each provider supplies its own SseEventParser implementation (stateful,
// to handle multi-chunk tool call accumulation etc.). The provider's stream()
// method then becomes two lines:
//
//   let stream = sse_stream(response, MyParser::default());
//   Ok(Box::pin(stream))
//
// This separates the "how do we read bytes" problem (here) from the
// "how do we interpret events" problem (in each provider).
// ============================================================================

use futures::{Stream, StreamExt};

use crate::common::StreamEvent;

// ── Trait ────────────────────────────────────────────────────────────────────

/// Parse one SSE frame into zero or more StreamEvents.
///
/// The parser owns whatever state it needs across frames (e.g. accumulated
/// tool-call JSON, current block type). `event` may be empty for providers
/// that omit the `event:` line (OpenAI sends only `data:` lines).
pub trait SseEventParser: Send + 'static {
    fn parse(&mut self, event: &str, data: &str) -> Vec<StreamEvent>;

    /// True once the terminal frame has been seen (message_stop / [DONE]).
    /// Used to distinguish a clean end-of-stream from a premature disconnect.
    fn is_done(&self) -> bool;
}

// ── Public entry point ───────────────────────────────────────────────────────

/// Wrap an HTTP response into a Stream<StreamEvent> using the given parser.
pub fn sse_stream<P>(
    response: reqwest::Response,
    parser: P,
) -> impl Stream<Item = Result<StreamEvent, anyhow::Error>> + Send
where
    P: SseEventParser,
{
    let byte_stream = response.bytes_stream();
    let state = ParseState {
        buffer: Vec::new(),
        pending: std::collections::VecDeque::new(),
        parser,
    };

    futures::stream::unfold(
        (byte_stream.boxed(), state),
        |(mut stream, mut state)| async move {
            loop {
                // Yield any buffered events before reading more bytes.
                if let Some(event) = state.pending.pop_front() {
                    return Some((Ok(event), (stream, state)));
                }

                match stream.next().await {
                    Some(Ok(bytes)) => {
                        state.buffer.extend_from_slice(&bytes);
                        while let Some((event, data)) = extract_frame(&mut state.buffer) {
                            for ev in state.parser.parse(&event, &data) {
                                state.pending.push_back(ev);
                            }
                        }
                    }
                    Some(Err(e)) => {
                        return Some((
                            Err(anyhow::anyhow!("SSE read error: {}", e)),
                            (stream, state),
                        ));
                    }
                    None => {
                        if !state.parser.is_done() {
                            return Some((
                                Err(anyhow::anyhow!("SSE stream ended unexpectedly")),
                                (stream, state),
                            ));
                        }
                        return None;
                    }
                }
            }
        },
    )
}

// ── Frame extraction ─────────────────────────────────────────────────────────

/// Internal state for the unfold closure.
struct ParseState<P: SseEventParser> {
    buffer: Vec<u8>,
    pending: std::collections::VecDeque<StreamEvent>,
    parser: P,
}

/// Extract one complete SSE frame from the byte buffer.
///
/// An SSE frame is terminated by a blank line (`\n\n`).
/// Returns `(event_type, data)` — `event_type` is empty when no `event:` line
/// is present (OpenAI-style).
fn extract_frame(buffer: &mut Vec<u8>) -> Option<(String, String)> {
    let text = String::from_utf8_lossy(buffer);
    let boundary = text.find("\n\n")?;
    let frame = text[..boundary].to_string();
    *buffer = buffer[boundary + 2..].to_vec();

    let mut event = String::new();
    let mut data = String::new();

    for line in frame.lines() {
        if let Some(v) = line.strip_prefix("event: ") {
            event = v.to_string();
        } else if let Some(v) = line.strip_prefix("data: ") {
            data = v.to_string();
        }
    }

    // Frames with no data are skipped (heartbeats, comments).
    if data.is_empty() {
        return None;
    }

    Some((event, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_frame_anthropic_style() {
        let mut buf = b"event: message_start\ndata: {\"x\":1}\n\nremaining".to_vec();
        let frame = extract_frame(&mut buf).unwrap();
        assert_eq!(frame.0, "message_start");
        assert_eq!(frame.1, "{\"x\":1}");
        assert_eq!(buf, b"remaining");
    }

    #[test]
    fn test_extract_frame_openai_style() {
        let mut buf = b"data: {\"choices\":[]}\n\n".to_vec();
        let frame = extract_frame(&mut buf).unwrap();
        assert_eq!(frame.0, "");
        assert_eq!(frame.1, "{\"choices\":[]}");
    }

    #[test]
    fn test_extract_frame_incomplete_returns_none() {
        let mut buf = b"event: x\ndata: {partial".to_vec();
        assert!(extract_frame(&mut buf).is_none());
        // Buffer is unchanged.
        assert_eq!(buf, b"event: x\ndata: {partial");
    }

    #[test]
    fn test_extract_frame_skips_empty_data() {
        // Heartbeat / comment-only frame.
        let mut buf = b"event: ping\n\ndata: real\n\n".to_vec();
        let frame = extract_frame(&mut buf);
        // The ping frame has no data — skipped.
        assert!(frame.is_none());
    }
}
