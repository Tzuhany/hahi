// ============================================================================
// Context Isolation — Sub-Agent Configuration
//
// When spawning a sub-agent, we inherit some things (providers, tools)
// and isolate others (messages, tokens, depth).
//
// Key: the query chain depth increments. This prevents runaway recursion:
//   depth 0 (user) → depth 1 (sub-agent) → depth 2 (sub-sub-agent)
//   If depth >= max_depth → refuse to spawn.
// ============================================================================

use std::sync::Arc;

use crate::core::r#loop::{LoopConfig, RunMode};
use crate::core::pipeline::CompressionPipeline;
use crate::multi::agent_def::AgentDef;
use crate::tool::ToolRegistry;
use crate::tool::definition::Tool;

/// Build a LoopConfig for a sub-agent.
///
/// Returns None if max depth is exceeded (prevents infinite recursion).
/// Cancel token handling lives in spawn.rs via LoopRuntime::cancel.child_token().
pub fn build_sub_agent_config(parent: &LoopConfig, def: &AgentDef) -> Option<LoopConfig> {
    // Depth check — prevent runaway nesting.
    if parent.chain.depth >= parent.chain.max_depth {
        tracing::warn!(
            depth = parent.chain.depth,
            max = parent.chain.max_depth,
            agent = def.name,
            "sub-agent spawn rejected: max depth exceeded"
        );
        return None;
    }

    tracing::debug!(
        agent = def.name.as_str(),
        description = def.description.as_str(),
        "spawning sub-agent"
    );

    let tool_registry = resolve_tool_registry(&parent.tool_registry, def);

    let model = if def.model == "inherit" || def.model.is_empty() {
        parent.model.clone()
    } else {
        def.model.clone()
    };

    Some(LoopConfig {
        provider: Arc::clone(&parent.provider),
        // Sub-agents inherit the same provider for compression.
        compression: CompressionPipeline::standard(Arc::clone(&parent.provider), None),
        tool_registry,
        system_prompt: parent.system_prompt.clone(),
        model,
        max_tokens: parent.max_tokens,
        temperature: parent.temperature,
        provider_extensions: parent.provider_extensions.clone(),
        context_window_tokens: parent.context_window_tokens,
        max_iterations: def.max_turns,
        cwd: parent.cwd.clone(),
        run_mode: RunMode::Execute,
        compact_threshold: parent.compact_threshold,

        // Increment depth, share chain_id.
        chain: parent.chain.child(),

        // Inherit permission and metrics from parent — sub-agents respect the
        // same permission rules and accumulate into the same metric counters.
        permission: Arc::clone(&parent.permission),
        metrics: Arc::clone(&parent.metrics),
    })
}

/// Resolve tool registry for a sub-agent.
///
/// Applies the agent definition's `tools` allowlist and `disallowed_tools`
/// blocklist. Returns the parent registry unchanged when no filtering is needed.
fn resolve_tool_registry(parent: &Arc<ToolRegistry>, def: &AgentDef) -> Arc<ToolRegistry> {
    let allow_all = def.tools.iter().any(|t| t == "*");

    // Fast path: no filtering needed.
    if allow_all && def.disallowed_tools.is_empty() {
        return Arc::clone(parent);
    }

    let filtered: Vec<Arc<dyn Tool>> = parent
        .all_tools()
        .filter(|t| {
            let name = t.name();
            // Blocklist takes priority over allowlist.
            if def
                .disallowed_tools
                .iter()
                .any(|d| d.eq_ignore_ascii_case(name))
            {
                return false;
            }
            // If allow_all, every non-blocked tool passes.
            if allow_all {
                return true;
            }
            // Otherwise the tool must appear in the explicit allowlist.
            def.tools.iter().any(|a| a.eq_ignore_ascii_case(name))
        })
        .collect();

    tracing::debug!(
        agent = def.name.as_str(),
        total = parent.len(),
        after_filter = filtered.len(),
        "sub-agent tool registry filtered"
    );

    Arc::new(ToolRegistry::new(filtered))
}
