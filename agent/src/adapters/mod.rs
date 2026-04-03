// ============================================================================
// Adapters — External Interfaces
//
// Adapters connect the agent runtime to the outside world: gRPC, LLM providers,
// MCP servers, persistent storage, and metrics backends.
// ============================================================================

pub mod grpc;
pub mod llm;
pub mod mcp;
pub mod metrics;
pub mod store;
