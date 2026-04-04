// ============================================================================
// Common Types — Domain Lingua Franca
//
// Zero-dependency foundation types used across every module in the agent.
// These types represent the agent's domain model: conversations, tokens,
// streaming events, tool contracts, and persistence snapshots.
//
// Nothing in this module imports from other agent modules.
// Everything else depends on common/.
//
// Dependency rule: common/ has NO internal dependencies.
//   common → kernel/runtime/systems/adapters
// ============================================================================

pub mod checkpoint;
pub mod ids;
pub mod message;
pub mod stream_event;
pub mod token;
pub mod tool_types;

// Re-export core types at the module level for ergonomic imports.
#[allow(unused_imports)]
pub use checkpoint::{Checkpoint, ForkOrigin, PendingControl};
#[allow(unused_imports)]
pub use ids::{AgentId, MemoryId, MessageId, ThreadId};
pub use message::{ContentBlock, Message, Role};
pub use stream_event::{StopReason, StreamEvent};
pub use token::TokenUsage;
#[allow(unused_imports)]
pub use tool_types::{Artifact, ArtifactContent, ToolContext, ToolInput, ToolOutput, ToolProgress};
