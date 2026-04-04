// ============================================================================
// Kernel — Execution Semantics
//
// The kernel is the agent's inner execution engine. It owns the LLM/tool loop,
// control-flow stops, context pressure management, compression, permissions,
// and hook evaluation.
//
// It does not own conversation data or external I/O policy. Those are provided
// by runtime assembly and adapter layers.
// ============================================================================

pub mod compression;
pub mod context;
pub mod control;
pub mod error_recovery;
pub mod event_bus;
pub mod hooks;
pub mod r#loop;
pub mod permission;
pub mod plan_mode;
pub mod xml;
