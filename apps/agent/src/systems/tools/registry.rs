// ============================================================================
// Tool Registry — Two-Tier Loading
//
// The registry holds all available tools and partitions them into two tiers:
//
//   Resident:  Full schema in the system prompt. Immediately callable.
//              These are high-frequency tools (search, fetch, query).
//
//   Deferred:  Only name appears in the prompt. LLM must ToolSearch to
//              get the full schema before invoking. These are low-frequency
//              tools (send email, code execution, cron).
//
// Why two tiers?
//   A system prompt with 30+ tool schemas costs thousands of tokens.
//   Most turns only use 2-3 tools. Deferring the rest saves ~60% of
//   tool-related prompt tokens without limiting capability — the LLM
//   discovers tools on demand via ToolSearch.
//
// This is Claude Code's "目录常驻, 正文按需" philosophy applied to tools.
// ============================================================================

use std::collections::HashMap;
use std::sync::Arc;

use crate::adapters::llm::ToolDefinition;
use crate::systems::tools::definition::Tool;

/// The tool registry, holding all tools partitioned by loading tier.
///
/// Constructed once at agent startup. Shared across sub-agents via Arc.
/// Immutable after construction — tools don't change during a session.
pub struct ToolRegistry {
    /// Tools whose full schema is in the system prompt.
    resident: Vec<Arc<dyn Tool>>,

    /// Tools whose name is listed but schema is deferred.
    deferred: Vec<Arc<dyn Tool>>,

    /// Name → tool lookup for execution (both tiers).
    by_name: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// Build a registry from a list of tools.
    ///
    /// Each tool's `should_defer()` method determines its tier.
    /// ToolSearch itself is always resident (needed to load everything else).
    pub fn new(tools: Vec<Arc<dyn Tool>>) -> Self {
        let mut resident = Vec::new();
        let mut deferred = Vec::new();
        let mut by_name = HashMap::new();

        for tool in tools {
            by_name.insert(tool.name().to_string(), Arc::clone(&tool));

            if tool.should_defer() {
                deferred.push(tool);
            } else {
                resident.push(tool);
            }
        }

        Self {
            resident,
            deferred,
            by_name,
        }
    }

    /// Tool definitions for the LLM API call.
    ///
    /// Resident tools get full schema. Deferred tools are excluded —
    /// their names are announced separately via `<system-reminder>`.
    pub fn api_tool_definitions(&self) -> Vec<ToolDefinition> {
        self.resident
            .iter()
            .map(|t| ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                input_schema: t.input_schema(),
            })
            .collect()
    }

    /// Names of deferred tools, for injection into the prompt.
    ///
    /// These are announced to the LLM in a `<system-reminder>`:
    /// "The following tools are available via ToolSearch: ..."
    pub fn deferred_tool_names(&self) -> Vec<&str> {
        self.deferred.iter().map(|t| t.name()).collect()
    }

    /// Look up a tool by name (either tier) for execution.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.by_name.get(name).cloned()
    }

    /// Search deferred tools by query.
    ///
    /// Supports three query modes (mirroring Claude Code's ToolSearch):
    ///   - `"select:Name1,Name2"` → exact match by name
    ///   - `"keyword1 keyword2"`  → scored keyword search
    ///   - `"+prefix rest"`       → require prefix in name, rank by rest
    ///
    /// Returns full ToolDefinitions so the LLM can invoke them.
    pub fn search(&self, query: &str, max_results: usize) -> Vec<ToolDefinition> {
        // Exact selection mode: "select:WebSearch,SendEmail"
        if let Some(names) = query.strip_prefix("select:") {
            let targets: Vec<&str> = names.split(',').map(|s| s.trim()).collect();
            return self
                .deferred
                .iter()
                .filter(|t| {
                    targets
                        .iter()
                        .any(|&name| t.name().eq_ignore_ascii_case(name))
                })
                .map(|t| ToolDefinition {
                    name: t.name().to_string(),
                    description: t.description().to_string(),
                    input_schema: t.input_schema(),
                })
                .collect();
        }

        // Prefix-required mode: "+slack send message"
        let (required_prefix, search_terms) = if let Some(rest) = query.strip_prefix('+') {
            let mut parts = rest.splitn(2, ' ');
            let prefix = parts.next().unwrap_or("");
            let terms = parts.next().unwrap_or("");
            (Some(prefix), terms)
        } else {
            (None, query)
        };

        // Score each deferred tool against the search terms.
        let keywords: Vec<&str> = search_terms.split_whitespace().collect();
        let mut scored: Vec<(&Arc<dyn Tool>, u32)> = self
            .deferred
            .iter()
            .filter(|t| {
                // If prefix required, tool name must contain it.
                required_prefix
                    .map(|p| t.name().to_lowercase().contains(&p.to_lowercase()))
                    .unwrap_or(true)
            })
            .map(|t| {
                let score = score_tool(t.as_ref(), &keywords);
                (t, score)
            })
            .filter(|(_, score)| *score > 0)
            .collect();

        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored.truncate(max_results);

        scored
            .into_iter()
            .map(|(t, _)| ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                input_schema: t.input_schema(),
            })
            .collect()
    }

    /// Iterator over every tool in both tiers.
    ///
    /// Used by sub-agent tool filtering to build a restricted registry.
    pub fn all_tools(&self) -> impl Iterator<Item = Arc<dyn Tool>> + '_ {
        self.resident.iter().chain(self.deferred.iter()).cloned()
    }

    /// Total number of tools (both tiers).
    pub fn len(&self) -> usize {
        self.by_name.len()
    }
}

// Score weights — tuned so exact name matches dominate over fuzzy description matches.
const SCORE_EXACT_NAME: u32 = 10;
const SCORE_NAME_CONTAINS: u32 = 5;
const SCORE_HINT_CONTAINS: u32 = 4;
const SCORE_DESC_CONTAINS: u32 = 2;

/// Score a tool against search keywords. Higher score = better match.
fn score_tool(tool: &dyn Tool, keywords: &[&str]) -> u32 {
    let name_lower = tool.name().to_lowercase();
    let desc_lower = tool.description().to_lowercase();
    let hint_lower = tool
        .search_hint()
        .map(|h| h.to_lowercase())
        .unwrap_or_default();

    let mut score = 0u32;

    for kw in keywords {
        let kw_lower = kw.to_lowercase();

        if name_lower == kw_lower {
            score += SCORE_EXACT_NAME;
        } else if name_lower.contains(&kw_lower) {
            score += SCORE_NAME_CONTAINS;
        }

        if hint_lower.contains(&kw_lower) {
            score += SCORE_HINT_CONTAINS;
        }

        if desc_lower.contains(&kw_lower) {
            score += SCORE_DESC_CONTAINS;
        }
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{ToolContext, ToolOutput};
    use async_trait::async_trait;
    use serde_json::json;

    /// Minimal test tool for registry tests.
    struct FakeTool {
        name: &'static str,
        description: &'static str,
        defer: bool,
        hint: Option<&'static str>,
    }

    #[async_trait]
    impl Tool for FakeTool {
        fn name(&self) -> &str {
            self.name
        }
        fn description(&self) -> &str {
            self.description
        }
        fn prompt(&self) -> String {
            String::new()
        }
        fn input_schema(&self) -> serde_json::Value {
            json!({})
        }
        fn should_defer(&self) -> bool {
            self.defer
        }
        fn search_hint(&self) -> Option<&str> {
            self.hint
        }
        async fn call(&self, _input: serde_json::Value, _ctx: &ToolContext) -> ToolOutput {
            ToolOutput::success("ok")
        }
    }

    fn test_registry() -> ToolRegistry {
        ToolRegistry::new(vec![
            Arc::new(FakeTool {
                name: "WebSearch",
                description: "Search the web",
                defer: false,
                hint: Some("google search"),
            }),
            Arc::new(FakeTool {
                name: "SendEmail",
                description: "Send an email",
                defer: true,
                hint: Some("mail smtp"),
            }),
            Arc::new(FakeTool {
                name: "DbQuery",
                description: "Query the database",
                defer: false,
                hint: None,
            }),
            Arc::new(FakeTool {
                name: "SlackNotify",
                description: "Send slack notification",
                defer: true,
                hint: Some("slack message"),
            }),
        ])
    }

    #[test]
    fn test_partitioning() {
        let reg = test_registry();
        assert_eq!(reg.api_tool_definitions().len(), 2); // WebSearch, DbQuery
        assert_eq!(reg.deferred_tool_names().len(), 2); // SendEmail, SlackNotify
    }

    #[test]
    fn test_get_by_name() {
        let reg = test_registry();
        assert!(reg.get("WebSearch").is_some());
        assert!(reg.get("SendEmail").is_some());
        assert!(reg.get("NonExistent").is_none());
    }

    #[test]
    fn test_search_select_mode() {
        let reg = test_registry();
        let results = reg.search("select:SendEmail,SlackNotify", 10);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_keyword_mode() {
        let reg = test_registry();
        let results = reg.search("slack message", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "SlackNotify");
    }

    #[test]
    fn test_search_prefix_mode() {
        let reg = test_registry();
        let results = reg.search("+slack notify", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "SlackNotify");
    }
}
