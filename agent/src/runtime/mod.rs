// ============================================================================
// Runtime — Turn Assembly and Runtime State
//
// Runtime bridges the pure execution kernel and the outer adapters. It assembles
// one turn's dependencies, builds the system prompt, persists runtime state, and
// runs post-turn policies such as reflection.
// ============================================================================

pub mod assembler;
pub mod builders;
pub mod prompt;
pub mod reflection_runner;
pub mod state;
