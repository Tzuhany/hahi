// ============================================================================
// Infrastructure Layer
//
// Groups all infrastructure concerns: database access, caching, and
// observability. Nothing in this module contains domain logic.
//
// Dependency rule: infra/ depends only on common/.
//   common → infra → llm/tool/memory/skill/mcp → core → service
// ============================================================================

pub mod metrics;
pub mod store;
