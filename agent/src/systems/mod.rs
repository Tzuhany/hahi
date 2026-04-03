// ============================================================================
// Systems — Agent Subsystems
//
// These modules are the agent's reusable internal subsystems: memory, tools,
// skills, and sub-agents. They are not transport adapters and they are not the
// core execution loop; they are the capabilities the runtime assembles around
// the kernel.
// ============================================================================

pub mod memory;
pub mod skills;
pub mod subagents;
pub mod tools;
