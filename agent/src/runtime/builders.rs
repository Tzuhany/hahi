use std::sync::{Arc, OnceLock};

use tokio::sync::Mutex;

use crate::runtime::prompt::builder::{PromptConfig, build_system_prompt_with_cache};
use crate::runtime::prompt::cache::PromptCache;
use crate::runtime::prompt::cache_boundary::join_sections;
use crate::systems::memory::MemoryCtx;
use crate::systems::skills::loader::SkillDef;
use crate::systems::tools::builtin::{
    AgentTool, MemoryForgetTool, MemorySearchTool, MemoryWriteTool, SpawnFn, WebFetchTool,
    WebSearchTool,
};
use crate::systems::tools::definition::Tool;
use crate::systems::tools::registry::ToolRegistry;
use crate::systems::tools::search::ToolSearchTool;

pub fn build_tool_registry(
    spawn_cell: Arc<OnceLock<SpawnFn>>,
    memory_ctx: Arc<MemoryCtx>,
    mcp_tools: &[Arc<dyn Tool>],
) -> Arc<ToolRegistry> {
    let mut searchable_tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(AgentTool::new(Arc::clone(&spawn_cell))),
        Arc::new(WebFetchTool::new()),
        Arc::new(WebSearchTool::new()),
        Arc::new(MemoryWriteTool::new(Arc::clone(&memory_ctx))),
        Arc::new(MemorySearchTool::new(Arc::clone(&memory_ctx))),
        Arc::new(MemoryForgetTool::new(Arc::clone(&memory_ctx))),
    ];
    searchable_tools.extend(mcp_tools.iter().cloned());

    let search_registry = Arc::new(ToolRegistry::new(searchable_tools.clone()));
    let mut tools = searchable_tools;
    tools.push(Arc::new(ToolSearchTool::new(search_registry)));

    Arc::new(ToolRegistry::new(tools))
}

pub async fn build_system_prompt(
    tool_registry: &ToolRegistry,
    skills: &[SkillDef],
    memory_index: Option<&str>,
    recalled_section: Option<&str>,
    write_guidance: &str,
    prompt_cache: &Arc<Mutex<PromptCache>>,
    context_window_tokens: usize,
    model_name: &str,
) -> String {
    let prompt_sections = {
        let mut cache = prompt_cache.lock().await;
        build_system_prompt_with_cache(
            &PromptConfig {
                tool_registry,
                skills,
                memory_index,
                user_instructions: "",
                project_instructions: "",
                rules: &[],
                context_window_tokens,
                model_name,
            },
            &mut cache,
        )
    };
    let mut system_prompt = join_sections(&prompt_sections);
    if let Some(recalled) = recalled_section {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(recalled);
    }
    system_prompt.push_str("\n\n");
    system_prompt.push_str(write_guidance);
    system_prompt
}
