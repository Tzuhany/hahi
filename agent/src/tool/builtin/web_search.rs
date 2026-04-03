// WebSearchTool — searches the web via the Tavily API.
//
// Requires TAVILY_API_KEY environment variable.
// Returns a direct answer (when available) plus ranked result snippets.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use crate::common::{ToolContext, ToolInput, ToolOutput};
use crate::tool::definition::Tool;

pub struct WebSearchTool {
    client: reqwest::Client,
}

impl WebSearchTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
        }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "WebSearch"
    }

    fn description(&self) -> &str {
        "Search the web for current information."
    }

    fn prompt(&self) -> String {
        "Searches the web using the Tavily API and returns a direct answer plus \
         ranked result snippets.\n\
         Requires TAVILY_API_KEY environment variable (free tier at https://tavily.com)."
            .to_string()
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query",
                    "minLength": 1
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum results to return (default 5, max 10)",
                    "minimum": 1,
                    "maximum": 10
                }
            },
            "required": ["query"]
        })
    }

    fn is_concurrent(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        if ctx.cancel.is_cancelled() {
            return ToolOutput::error("cancelled");
        }

        let api_key = match std::env::var("TAVILY_API_KEY") {
            Ok(k) if !k.is_empty() => k,
            _ => {
                return ToolOutput::error(
                    "WebSearch requires TAVILY_API_KEY environment variable.\n\
                     Get a free API key at https://tavily.com",
                );
            }
        };

        let inp = ToolInput(&input);
        let query = match inp.required_str("query") {
            Ok(q) => q.to_string(),
            Err(e) => return ToolOutput::error(e),
        };
        let max_results = inp.optional_u64("max_results").unwrap_or(5).min(10);

        let resp = match self
            .client
            .post("https://api.tavily.com/search")
            .json(&serde_json::json!({
                "api_key": api_key,
                "query": query,
                "max_results": max_results,
                "search_depth": "basic",
                "include_answer": true,
            }))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolOutput::error(format!("search request failed: {e}")),
        };

        if !resp.status().is_success() {
            return ToolOutput::error(format!("Tavily API returned {}", resp.status()));
        }

        let body: Value = match resp.json().await {
            Ok(j) => j,
            Err(e) => return ToolOutput::error(format!("failed to parse search response: {e}")),
        };

        let mut out = String::new();

        if let Some(answer) = body["answer"].as_str() {
            out.push_str(&format!("**Direct answer:** {answer}\n\n"));
        }

        if let Some(results) = body["results"].as_array() {
            out.push_str(&format!("**Results for \"{}\":**\n\n", query));
            for (i, r) in results.iter().enumerate() {
                let title = r["title"].as_str().unwrap_or("(no title)");
                let url = r["url"].as_str().unwrap_or("");
                let snippet = r["content"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(400)
                    .collect::<String>();
                out.push_str(&format!(
                    "{}. **{}**\n   {}\n   {}\n\n",
                    i + 1,
                    title,
                    url,
                    snippet
                ));
            }
        }

        if out.is_empty() {
            return ToolOutput::error(
                "no results returned — try a broader or different search query",
            );
        }

        // Cap total output to protect the context window.
        const MAX_OUTPUT: usize = 6_000;
        let out = if out.chars().count() > MAX_OUTPUT {
            let truncated: String = out.chars().take(MAX_OUTPUT).collect();
            format!(
                "{}\n\n[truncated — showing {} of {} chars]",
                truncated,
                MAX_OUTPUT,
                out.len()
            )
        } else {
            out
        };

        ToolOutput::success(out)
    }
}
