// WebFetchTool — fetches the raw content of a URL.
//
// Returns the response body as-is (HTML is not stripped).
// Content is truncated at max_chars to protect the context window.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use crate::common::{ToolContext, ToolInput, ToolOutput};
use crate::systems::tools::definition::Tool;

/// Default character limit for fetched content (~5k tokens).
const MAX_CHARS: usize = 20_000;

pub struct WebFetchTool {
    client: reqwest::Client,
}

impl WebFetchTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("hahi-agent/0.1 (compatible)")
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "WebFetch"
    }

    fn description(&self) -> &str {
        "Fetch the raw content of a URL."
    }

    fn prompt(&self) -> String {
        "Fetches the content of a URL and returns it as text.\n\
         HTML pages are returned as-is; JSON APIs return the raw response body.\n\
         Use `max_chars` to limit how much content is returned (default 20000)."
            .to_string()
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch (http or https)",
                    "minLength": 1
                },
                "max_chars": {
                    "type": "integer",
                    "description": "Maximum characters to return (default 20000, max 100000)",
                    "minimum": 1,
                    "maximum": 100000
                }
            },
            "required": ["url"]
        })
    }

    fn is_concurrent(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let inp = ToolInput(&input);
        let url = match inp.required_str("url") {
            Ok(u) => u.to_string(),
            Err(e) => return ToolOutput::error(e),
        };
        let max_chars = inp
            .optional_u64("max_chars")
            .unwrap_or(MAX_CHARS as u64)
            .min(100_000) as usize;

        if ctx.cancel.is_cancelled() {
            return ToolOutput::error("cancelled");
        }

        let response = match self.client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => return ToolOutput::error(format!("failed to fetch {url}: {e}")),
        };

        let status = response.status();
        if !status.is_success() {
            return ToolOutput::error(format!("HTTP {status} from {url}"));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string();

        let body = match response.text().await {
            Ok(t) => t,
            Err(e) => return ToolOutput::error(format!("failed to read response: {e}")),
        };

        // Truncate at char boundaries to avoid panicking on multi-byte characters.
        let total_chars = body.chars().count();
        let (body, truncation_note) = if total_chars > max_chars {
            let truncated: String = body.chars().take(max_chars).collect();
            (
                truncated,
                Some(format!(
                    "\n\n[truncated — showing {max_chars} of {total_chars} chars]"
                )),
            )
        } else {
            (body, None)
        };

        let mut out = format!("URL: {url}\nContent-Type: {content_type}\n\n{body}");
        if let Some(note) = truncation_note {
            out.push_str(&note);
        }
        ToolOutput::success(out)
    }
}
