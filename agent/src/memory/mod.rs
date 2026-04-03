// Memory system entry point.
//
// Module map:
//   types     — core types: Memory, WriteRequest, WriteStatus, RecallResult, SessionStats
//   policy    — write validation: size limits, empty checks, whitespace normalization
//   embed     — EmbeddingProvider trait + NoOpEmbedder fallback
//   recall    — per-turn retrieval: pinned (unconditional) + RRF (conditional)
//   lifecycle — session-end maintenance: stale retirement, importance decay/boost
//   inject    — format index and recalled memories for LLM prompt injection
//   reflect   — post-run reflection: decide when + build reflection prompt
//
// Store operations (all SQL) live in infra/store/pg/memory.rs.
// Tools (MemoryWrite, MemorySearch, MemoryForget) live in tool/builtin/memory_*.rs.

pub mod ctx;
pub mod embed;
pub mod engine;
pub mod inject;
pub mod lifecycle;
pub mod policy;
pub mod recall;
pub mod reflect;
pub mod types;

#[allow(unused_imports)]
pub use ctx::MemoryCtx;
#[allow(unused_imports)]
pub use engine::{MemoryEngine, TurnMemory};
