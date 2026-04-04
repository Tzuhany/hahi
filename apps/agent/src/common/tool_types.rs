// ============================================================================
// Tool Types — Cross-Module Tool Contracts
//
// Types used by multiple modules to describe tool invocations, results,
// and artifacts. Extracted from systems/tools/definition.rs to break the dependency
// cycle where tool/ would otherwise need to import from llm/ for Message.
//
// The Tool trait itself stays in systems/tools/definition.rs — it depends on these
// types but not the other way around.
// ============================================================================

use serde::{Deserialize, Serialize};

use crate::common::message::Message;

// ============================================================================
// ToolInput — ergonomic field extraction
//
// Every tool's call() method receives a raw serde_json::Value. Extracting
// required/optional fields with good error messages is tedious and repetitive.
// ToolInput wraps the value with typed accessors so each extraction is one line.
//
//   let inp = ToolInput(&input);
//   let Ok(url)   = inp.required_str("url") else { return ToolOutput::error(...) };
//   let max_chars = inp.optional_u64("max_chars").unwrap_or(20_000);
// ============================================================================

/// Ergonomic wrapper around a JSON tool input.
pub struct ToolInput<'a>(pub &'a serde_json::Value);

impl<'a> ToolInput<'a> {
    /// Extract a required non-empty string field.
    ///
    /// Returns `Err("<field> is required")` when missing or empty.
    pub fn required_str(&self, key: &str) -> Result<&'a str, String> {
        match self.0[key].as_str().filter(|s| !s.is_empty()) {
            Some(s) => Ok(s),
            None => Err(format!("{key} is required")),
        }
    }

    /// Extract an optional string field. Returns `None` when missing or null.
    #[allow(dead_code)]
    pub fn optional_str(&self, key: &str) -> Option<&'a str> {
        self.0[key].as_str()
    }

    /// Extract a required boolean field.
    #[allow(dead_code)]
    pub fn required_bool(&self, key: &str) -> Result<bool, String> {
        self.0[key]
            .as_bool()
            .ok_or_else(|| format!("{key} is required (boolean)"))
    }

    /// Extract an optional boolean field.
    #[allow(dead_code)]
    pub fn optional_bool(&self, key: &str) -> Option<bool> {
        self.0[key].as_bool()
    }

    /// Extract a required signed integer field.
    #[allow(dead_code)]
    pub fn required_i64(&self, key: &str) -> Result<i64, String> {
        self.0[key]
            .as_i64()
            .ok_or_else(|| format!("{key} is required (integer)"))
    }

    /// Extract an optional signed integer field.
    #[allow(dead_code)]
    pub fn optional_i64(&self, key: &str) -> Option<i64> {
        self.0[key].as_i64()
    }

    /// Extract an optional unsigned integer field.
    pub fn optional_u64(&self, key: &str) -> Option<u64> {
        self.0[key].as_u64()
    }
}

/// Context passed to every tool invocation.
///
/// Provides everything the tool needs without coupling it to the agent's
/// internals. Tools never touch the message history, LLM client, or session.
pub struct ToolContext {
    /// Working directory for file operations.
    /// Used by filesystem tools (not yet implemented).
    #[allow(dead_code)]
    pub cwd: std::path::PathBuf,

    /// Cancellation signal. Tools should check periodically and bail early.
    pub cancel: tokio_util::sync::CancellationToken,

    /// Snapshot of the parent message history at the moment this tool started.
    /// Used by delegation tools that need fork-aware context construction.
    pub message_history: Vec<Message>,

    /// Callback to report intermediate progress.
    pub on_progress: Box<dyn Fn(ToolProgress) + Send + Sync>,
}

/// Progress update from a long-running tool.
#[derive(Debug, Clone)]
pub struct ToolProgress {
    pub message: String,
}

/// An artifact produced by a tool — files, images, structured data.
///
/// Artifacts are separate from the text content. They're delivered to the
/// client for rendering (e.g., a chart, a generated file, a code snippet)
/// and optionally stored for later reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    /// What kind of artifact: "file", "image", "chart", "code", etc.
    pub kind: String,

    /// Display title.
    pub title: String,

    /// MIME type (e.g., "text/plain", "image/png", "application/json").
    pub mime_type: String,

    /// The content — either inline text or a reference (URL/path).
    pub content: ArtifactContent,
}

/// Artifact content: either inline data or a reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ArtifactContent {
    /// Inline text content (code, JSON, markdown, etc.).
    Inline { data: String },

    /// Reference to external storage (S3 URL, file path).
    Reference { uri: String },
}

/// The result of a tool invocation.
///
/// Richer than a plain string — tools can:
///   - Return text content (shown to LLM as tool_result)
///   - Inject extra messages into the conversation (e.g., system context)
///   - Produce artifacts (files, images — delivered to client)
///   - Signal error (LLM sees the error and decides how to recover)
#[derive(Debug, Clone)]
pub struct ToolOutput {
    /// Primary text content, returned to the LLM as tool_result.
    pub content: String,

    /// Whether the tool execution failed.
    /// Failed results are still sent to the LLM — it decides recovery.
    pub is_error: bool,

    /// Extra messages to inject into the conversation after this tool result.
    ///
    /// Use case: a tool that loads context (e.g., "ReadDocumentation" might
    /// inject a system-reminder with the doc content, separate from the
    /// tool_result itself).
    pub extra_messages: Vec<Message>,

    /// Artifacts produced by the tool (files, images, structured data).
    ///
    /// Delivered to the client for rendering. The LLM sees a mention in
    /// the tool_result content, but the actual artifact data goes to the
    /// client separately.
    pub artifacts: Vec<Artifact>,
}

impl ToolOutput {
    /// Simple success with text content only.
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
            extra_messages: Vec::new(),
            artifacts: Vec::new(),
        }
    }

    /// Simple error.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: message.into(),
            is_error: true,
            extra_messages: Vec::new(),
            artifacts: Vec::new(),
        }
    }

    /// Success with artifacts attached.
    pub fn with_artifacts(mut self, artifacts: Vec<Artifact>) -> Self {
        self.artifacts = artifacts;
        self
    }

    /// Attach extra messages to inject into the conversation.
    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.extra_messages = messages;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_output_success() {
        let output = ToolOutput::success("found 3 results");
        assert!(!output.is_error);
        assert_eq!(output.content, "found 3 results");
        assert!(output.extra_messages.is_empty());
        assert!(output.artifacts.is_empty());
    }

    #[test]
    fn test_tool_output_error() {
        let output = ToolOutput::error("connection refused");
        assert!(output.is_error);
    }

    #[test]
    fn test_tool_output_with_artifacts() {
        let artifact = Artifact {
            kind: "file".into(),
            title: "output.csv".into(),
            mime_type: "text/csv".into(),
            content: ArtifactContent::Inline {
                data: "a,b,c\n1,2,3".into(),
            },
        };

        let output = ToolOutput::success("Generated CSV file.").with_artifacts(vec![artifact]);

        assert_eq!(output.artifacts.len(), 1);
        assert_eq!(output.artifacts[0].kind, "file");
    }

    #[test]
    fn test_tool_output_with_extra_messages() {
        let msg = Message::user("sys-1", "extra context");
        let output = ToolOutput::success("done").with_messages(vec![msg]);

        assert_eq!(output.extra_messages.len(), 1);
    }
}
