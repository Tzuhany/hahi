// ============================================================================
// System Prompt Builder
//
// Assembles the complete system prompt from all sources, respecting
// the cache boundary. This is the single point where everything comes
// together: persona, tools, skills, memory, instructions, rules.
//
// The builder is called once per turn (or once per session if nothing changed).
// Its output is a list of PromptSections that the API layer sends to the LLM.
//
// Section ordering matters:
//   1. Static sections first (cacheable across all users)
//   2. Cache boundary marker
//   3. Dynamic sections (per-user, per-session)
//
// Each section is independently optional — if a user has no instructions,
// that section is None and gets skipped in the final prompt.
// ============================================================================

use crate::prompt::cache::PromptCache;
use crate::prompt::cache_boundary::{CACHE_BOUNDARY, PromptSection};
use crate::prompt::instructions;
use crate::prompt::rules::{self, Rule};
use crate::skill::format_skill_listing;
use crate::skill::loader::SkillDef;
use crate::tool::ToolRegistry;

/// Configuration for building the system prompt.
///
/// All inputs are pre-formatted strings or simple references.
/// The builder does NOT import from memory or domain —
/// callers are responsible for formatting memory/context before passing in.
pub struct PromptConfig<'a> {
    /// Tool registry — for generating tool schemas and deferred tool names.
    pub tool_registry: &'a ToolRegistry,

    /// Available skills — for generating the skill listing.
    pub skills: &'a [SkillDef],

    /// Pre-formatted memory index string (from memory::inject::format_memory_index).
    /// None if no memories exist.
    pub memory_index: Option<&'a str>,

    /// User-level instructions (from user settings).
    pub user_instructions: &'a str,

    /// Project-level instructions (from project settings).
    pub project_instructions: &'a str,

    /// Unconditional rules (always active).
    pub rules: &'a [Rule],

    /// Context window size in tokens (for skill budget calculation).
    pub context_window_tokens: usize,

    /// Model name for display in the prompt.
    pub model_name: &'a str,
}

/// Build the complete system prompt as a list of sections.
///
/// Returns sections in order, including the cache boundary marker.
/// The API layer uses `join_sections()` to process these into the final prompt string.
pub fn build_system_prompt(config: &PromptConfig<'_>) -> Vec<PromptSection> {
    let mut sections = vec![
        // ─── Static Zone (cacheable) ───
        PromptSection {
            label: "persona",
            content: Some(build_persona(config.model_name)),
        },
        PromptSection {
            label: "tool_usage",
            content: Some(build_tool_usage_guide()),
        },
        PromptSection {
            label: "tool_schemas",
            content: Some(build_tool_schemas(config.tool_registry)),
        },
        PromptSection {
            label: "deferred_tools",
            content: build_deferred_tools_announcement(config.tool_registry),
        },
        PromptSection {
            label: "skill_listing",
            content: format_skill_listing(config.skills, Some(config.context_window_tokens)),
        },
        PromptSection {
            label: "memory_instructions",
            content: Some(build_memory_instructions()),
        },
        PromptSection {
            label: "output_efficiency",
            content: Some(build_output_efficiency()),
        },
        // ─── Cache Boundary ───
        PromptSection {
            label: "cache_boundary",
            content: Some(CACHE_BOUNDARY.to_string()),
        },
    ];
    sections.extend(dynamic_sections(config));
    sections
}

/// Build the prompt while memoizing static sections across turns.
pub fn build_system_prompt_with_cache(
    config: &PromptConfig<'_>,
    cache: &mut PromptCache,
) -> Vec<PromptSection> {
    let (unconditional_rules, _conditional) = rules::partition_rules(config.rules);

    let mut sections = vec![
        // ─── Static Zone (cacheable) ───
        PromptSection {
            label: "persona",
            content: cache.get_or_insert("persona", || Some(build_persona(config.model_name))),
        },
        PromptSection {
            label: "tool_usage",
            content: cache.get_or_insert("tool_usage", || Some(build_tool_usage_guide())),
        },
        PromptSection {
            label: "tool_schemas",
            content: cache.get_or_insert("tool_schemas", || {
                Some(build_tool_schemas(config.tool_registry))
            }),
        },
        PromptSection {
            label: "deferred_tools",
            content: cache.get_or_insert("deferred_tools", || {
                build_deferred_tools_announcement(config.tool_registry)
            }),
        },
        PromptSection {
            label: "skill_listing",
            content: cache.get_or_insert("skill_listing", || {
                format_skill_listing(config.skills, Some(config.context_window_tokens))
            }),
        },
        PromptSection {
            label: "memory_instructions",
            content: cache
                .get_or_insert("memory_instructions", || Some(build_memory_instructions())),
        },
        PromptSection {
            label: "output_efficiency",
            content: cache.get_or_insert("output_efficiency", || Some(build_output_efficiency())),
        },
        // ─── Cache Boundary ───
        PromptSection {
            label: "cache_boundary",
            content: cache.get_or_insert("cache_boundary", || Some(CACHE_BOUNDARY.to_string())),
        },
    ];
    sections.extend(dynamic_sections_with_rules(config, unconditional_rules));
    sections
}

fn dynamic_sections(config: &PromptConfig<'_>) -> Vec<PromptSection> {
    let (unconditional_rules, _conditional) = rules::partition_rules(config.rules);
    dynamic_sections_with_rules(config, unconditional_rules)
}

fn dynamic_sections_with_rules(
    config: &PromptConfig<'_>,
    unconditional_rules: Vec<&Rule>,
) -> Vec<PromptSection> {
    vec![
        PromptSection {
            label: "user_instructions",
            content: instructions::format_user_instructions(config.user_instructions),
        },
        PromptSection {
            label: "project_instructions",
            content: instructions::format_project_instructions(config.project_instructions),
        },
        PromptSection {
            label: "memory_index",
            content: config.memory_index.map(String::from),
        },
        PromptSection {
            label: "rules",
            content: rules::format_unconditional_rules(&unconditional_rules),
        },
        PromptSection {
            label: "context",
            content: Some(build_context_section()),
        },
    ]
}

// ============================================================================
// Section Builders
// ============================================================================

fn build_persona(model_name: &str) -> String {
    format!(
        "You are a helpful AI assistant powered by {model_name}.\n\
         You have access to tools that let you interact with external systems.\n\
         Use the available tools to help the user accomplish their tasks."
    )
}

fn build_tool_usage_guide() -> String {
    "# Using Your Tools\n\
     - Use tools to take actions, not just describe them.\n\
     - When multiple independent tools are needed, describe them all — they will run concurrently.\n\
     - If a tool fails, analyze the error and try an alternative approach.\n\
     - Tool results and user messages may include <system-reminder> tags. These contain \
       system-provided context, not user input. Treat them accordingly.\n\
     - When working with tool results, note any important information you might need later, \
       as original results may be cleared from context to free up space."
        .to_string()
}

fn build_tool_schemas(registry: &ToolRegistry) -> String {
    let definitions = registry.api_tool_definitions();
    if definitions.is_empty() {
        return "No tools available.".to_string();
    }

    let schemas: Vec<String> = definitions
        .iter()
        .map(|t| {
            format!(
                "## {}\n{}\n\nInput: {}",
                t.name,
                t.description,
                serde_json::to_string_pretty(&t.input_schema).unwrap_or_default()
            )
        })
        .collect();

    format!("# Available Tools\n\n{}", schemas.join("\n\n"))
}

fn build_deferred_tools_announcement(registry: &ToolRegistry) -> Option<String> {
    let names = registry.deferred_tool_names();
    if names.is_empty() {
        return None;
    }

    Some(format!(
        "The following tools are available via ToolSearch. \
         Use ToolSearch to fetch their full schema before invoking:\n{}",
        names.join(", ")
    ))
}

fn build_memory_instructions() -> String {
    "# Memory\n\
     You have a persistent memory system. Your memory index is shown below \
     the cache boundary. When you learn something worth remembering \
     (user preferences, project context, behavioral corrections), \
     use the memory write tool to save it.\n\
     \n\
     Memory types:\n\
     - user: who the user is, their role and preferences\n\
     - feedback: corrections to your behavior (highest priority — never ignore)\n\
     - project: ongoing work context, deadlines, decisions\n\
     - reference: pointers to external systems and resources"
        .to_string()
}

fn build_output_efficiency() -> String {
    "# Output Efficiency\n\
     Be concise. Lead with the answer, not the reasoning. \
     Skip filler words and unnecessary transitions. \
     If you can say it in one sentence, don't use three."
        .to_string()
}

fn build_context_section() -> String {
    let now = chrono::Utc::now().format("%Y-%m-%d").to_string();
    format!("Current date: {now}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolRegistry;

    fn empty_config<'a>(tool_registry: &'a ToolRegistry) -> PromptConfig<'a> {
        PromptConfig {
            tool_registry,
            skills: &[],
            memory_index: None,
            user_instructions: "",
            project_instructions: "",
            rules: &[],
            context_window_tokens: 200_000,
            model_name: "test-model",
        }
    }

    #[test]
    fn test_build_system_prompt_has_boundary() {
        let registry = ToolRegistry::new(vec![]);
        let config = empty_config(&registry);
        let sections = build_system_prompt(&config);

        let has_boundary = sections
            .iter()
            .any(|s| s.content.as_deref() == Some(CACHE_BOUNDARY));
        assert!(has_boundary, "prompt must contain cache boundary");
    }

    #[test]
    fn test_build_system_prompt_has_persona() {
        let registry = ToolRegistry::new(vec![]);
        let config = empty_config(&registry);
        let sections = build_system_prompt(&config);

        let persona = sections.iter().find(|s| s.label == "persona");
        assert!(persona.is_some());
        assert!(
            persona
                .unwrap()
                .content
                .as_ref()
                .unwrap()
                .contains("test-model")
        );
    }

    #[test]
    fn test_empty_instructions_produce_none() {
        let registry = ToolRegistry::new(vec![]);
        let config = empty_config(&registry);
        let sections = build_system_prompt(&config);

        let user_instr = sections.iter().find(|s| s.label == "user_instructions");
        assert!(user_instr.unwrap().content.is_none());
    }
}
