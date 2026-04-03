// ============================================================================
// Agent Definitions
//
// Each sub-agent type is defined by a YAML file in subagents/agents/.
// The definition controls:
//   - Which model to use (or "inherit" from parent)
//   - Which tools are available
//   - Maximum number of LLM calls (safety limit)
//   - Description (for logging and sub-agent notifications)
//
// Built-in agent types follow Claude Code's pattern:
//   - general:  full capability, inherits parent model
//   - explorer: read-only tools, cheap model (for research)
//   - planner:  read-only tools, cheap model (for planning)
//
// Custom agent types can be added by dropping a YAML file in the agents/ dir.
// ============================================================================

use serde::Deserialize;

/// An agent type definition, loaded from YAML.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentDef {
    /// Unique name of this agent type.
    pub name: String,

    /// What this agent type does. Shown in task notifications.
    pub description: String,

    /// Model to use. "inherit" means use the parent's model.
    #[serde(default = "default_model")]
    pub model: String,

    /// Available tools. ["*"] means all tools.
    /// A specific list restricts the agent to only those tools.
    #[serde(default = "default_tools")]
    pub tools: Vec<String>,

    /// Tools to exclude (only relevant when tools is ["*"]).
    #[serde(default)]
    pub disallowed_tools: Vec<String>,

    /// Maximum LLM calls per turn. Safety limit.
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,
}

fn default_model() -> String {
    "inherit".into()
}

fn default_tools() -> Vec<String> {
    vec!["*".into()]
}

fn default_max_turns() -> u32 {
    50
}

/// Load built-in agent definitions.
///
/// These are compiled into the binary. Custom definitions can be added
/// at runtime by loading from a configurable directory.
pub fn builtin_agent_defs() -> Vec<AgentDef> {
    let defs = [
        include_str!("agents/general.yaml"),
        include_str!("agents/explorer.yaml"),
        include_str!("agents/planner.yaml"),
    ];

    defs.iter()
        .filter_map(|yaml| {
            serde_yaml::from_str::<AgentDef>(yaml)
                .map_err(|e| tracing::warn!(error = %e, "failed to parse agent definition"))
                .ok()
        })
        .collect()
}

/// Find an agent definition by name.
pub fn find_agent_def(name: &str) -> Option<AgentDef> {
    builtin_agent_defs()
        .into_iter()
        .find(|d| d.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_defs_load() {
        let defs = builtin_agent_defs();
        assert!(
            defs.len() >= 3,
            "should have at least general, explorer, planner"
        );

        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"general"));
        assert!(names.contains(&"explorer"));
        assert!(names.contains(&"planner"));
    }

    #[test]
    fn test_explorer_uses_haiku() {
        let explorer = find_agent_def("explorer").unwrap();
        assert_eq!(explorer.model, "haiku");
    }

    #[test]
    fn test_general_inherits_model() {
        let general = find_agent_def("general").unwrap();
        assert_eq!(general.model, "inherit");
    }

    #[test]
    fn test_find_nonexistent() {
        assert!(find_agent_def("nonexistent").is_none());
    }
}
