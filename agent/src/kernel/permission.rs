// ============================================================================
// Permission System
//
// Controls whether a tool can execute. Three modes:
//   Auto  — always allow (trusted environment, e.g., internal devops)
//   Ask   — pause and request user approval via ControlRequest event
//   Deny  — always reject
//
// Permissions are evaluated per tool, optionally per input pattern.
// Rules are ordered: first match wins.
//
// Integration with hooks:
//   Permissions are checked BEFORE PreToolUse hooks.
//   If permission is denied, the tool doesn't execute and hooks don't fire.
//   If permission requires approval, the run pauses (requires_action state).
//
// This is the cloud equivalent of Claude Code's permission modes
// (acceptEdits, bypassPermissions, default, plan, auto).
// ============================================================================

use serde::{Deserialize, Serialize};

/// Permission mode for the entire session.
///
/// Set at run creation time. Can be overridden per-tool via rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// All tools auto-approved. For trusted/internal environments.
    Auto,
    /// Tools require explicit user approval (default for external users).
    Ask,
    /// All tools denied except read-only. For preview/demo environments.
    Readonly,
}

impl Default for PermissionMode {
    fn default() -> Self {
        PermissionMode::Ask
    }
}

/// A permission rule: matches a tool (optionally with input pattern) to a decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    /// Tool name pattern. "*" matches all. "Bash" matches exact. "Bash(git *)" matches with args.
    pub tool_pattern: String,

    /// What to do when matched.
    pub decision: PermissionDecision,
}

/// The outcome of a permission check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    /// Tool execution allowed without asking.
    Allow,

    /// Pause the run and ask the user.
    /// The run enters `requires_action` state until the user responds.
    Ask,

    /// Tool execution denied. Error returned to LLM.
    Deny,
}

/// Evaluates permissions for a tool invocation.
pub struct PermissionEvaluator {
    mode: PermissionMode,
    rules: Vec<PermissionRule>,
}

impl PermissionEvaluator {
    pub fn new(mode: PermissionMode, rules: Vec<PermissionRule>) -> Self {
        Self { mode, rules }
    }

    /// Auto-approve everything. For trusted environments.
    pub fn auto() -> Self {
        Self::new(PermissionMode::Auto, Vec::new())
    }

    /// Check whether a tool is allowed to execute.
    ///
    /// Evaluation order:
    ///   1. Check per-tool rules (first match wins)
    ///   2. Fall back to session-wide mode
    pub fn check(&self, tool_name: &str, _input: &serde_json::Value) -> PermissionDecision {
        // Check rules first.
        for rule in &self.rules {
            if matches_pattern(&rule.tool_pattern, tool_name) {
                return rule.decision.clone();
            }
        }

        // Fall back to session mode.
        match self.mode {
            PermissionMode::Auto => PermissionDecision::Allow,
            PermissionMode::Ask => PermissionDecision::Ask,
            PermissionMode::Readonly => {
                // In readonly mode, only read-like tools are allowed.
                if is_readonly_tool(tool_name) {
                    PermissionDecision::Allow
                } else {
                    PermissionDecision::Deny
                }
            }
        }
    }
}

/// Check if a tool name matches a pattern.
///
/// Patterns:
///   "*"        → matches everything
///   "Bash"     → exact match
///   "Bash(*)"  → matches Bash with any args (future: arg inspection)
fn matches_pattern(pattern: &str, tool_name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if pattern.ends_with("(*)") {
        let prefix = &pattern[..pattern.len() - 3];
        return tool_name == prefix;
    }
    pattern == tool_name
}

/// Heuristic: is this tool read-only?
/// Used by Readonly mode to auto-allow safe tools.
///
/// Delegates to the plan_mode allow-list so both modes share one source of truth.
fn is_readonly_tool(name: &str) -> bool {
    crate::kernel::plan_mode::PLAN_MODE_TOOLS
        .iter()
        .any(|t| t.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_mode_always_allows() {
        let eval = PermissionEvaluator::auto();
        let decision = eval.check("Bash", &serde_json::json!({}));
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[test]
    fn test_ask_mode_defaults_to_ask() {
        let eval = PermissionEvaluator::new(PermissionMode::Ask, vec![]);
        let decision = eval.check("Bash", &serde_json::json!({}));
        assert_eq!(decision, PermissionDecision::Ask);
    }

    #[test]
    fn test_readonly_mode_allows_reads() {
        let eval = PermissionEvaluator::new(PermissionMode::Readonly, vec![]);
        assert_eq!(
            eval.check("WebSearch", &serde_json::json!({})),
            PermissionDecision::Allow
        );
        assert_eq!(
            eval.check("Bash", &serde_json::json!({})),
            PermissionDecision::Deny
        );
    }

    #[test]
    fn test_rule_overrides_mode() {
        let eval = PermissionEvaluator::new(
            PermissionMode::Ask,
            vec![PermissionRule {
                tool_pattern: "WebSearch".into(),
                decision: PermissionDecision::Allow,
            }],
        );
        // WebSearch matched by rule → Allow (not Ask).
        assert_eq!(
            eval.check("WebSearch", &serde_json::json!({})),
            PermissionDecision::Allow
        );
        // Bash not matched by rule → falls through to Ask.
        assert_eq!(
            eval.check("Bash", &serde_json::json!({})),
            PermissionDecision::Ask
        );
    }

    #[test]
    fn test_wildcard_rule() {
        let eval = PermissionEvaluator::new(
            PermissionMode::Ask,
            vec![PermissionRule {
                tool_pattern: "*".into(),
                decision: PermissionDecision::Allow,
            }],
        );
        assert_eq!(
            eval.check("AnyTool", &serde_json::json!({})),
            PermissionDecision::Allow
        );
    }
}
