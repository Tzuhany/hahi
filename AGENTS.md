# Hahi Agent — Architecture & Design Guide

## Project Overview

Hahi is a cloud-native distributed AI agent platform. The agent is a gRPC service that executes LLM + tool loops, streams results via Redis Stream, and persists state in PostgreSQL.

**Not a coding agent.** This is a general-purpose agent — could be DevOps, video generation, data analysis, or anything. Design decisions must be domain-agnostic.

## Architecture

```
Client ──HTTP/SSE──→ Gateway ──gRPC──→ Conversation Module ──gRPC──→ Agent
                       ↑                                              │
                       │              Redis Stream (events)           │
                       └──────────────────────────────────────────────┘

Agent connects: PG (checkpoints, memories, tool results, audit) + Redis (cache, events) + LLM API + disk (skills)
Gateway connects: Redis (subscribe events → SSE push)
Conversation Module: owns Thread/Run/Message/RunStep lifecycle (NOT in agent)
```

## Design Philosophy (from Codex)

1. **Framework is executor, not thinker** — LLM decides what to do via native tool_use + extended thinking. No ReAct text parsing, no framework-level reasoning.

2. **Trust the model** — Errors go back to the LLM as tool_result. LLM decides recovery. Framework only intervenes for structural failures (context overflow, rate limits).

3. **目录常驻, 正文按需** (index always present, content on demand) — Tool schemas, skill descriptions, memory index are lightweight summaries in the prompt. Full content loaded only when needed.

4. **Three-level compression** — L1: tool result truncation (free). L2: context collapse (free, reversible). L3: LLM summarization (costs tokens, irreversible).

5. **Sub-agents are async tasks, not processes** — `tokio::spawn`, not child processes. Share providers via Arc, isolate messages and tokens.

## Conversation Model

```
Thread  — persistent conversation (Conversation module owns this)
Run     — one execution cycle (Conversation module owns this)
RunStep — granular event log (Conversation module owns this)

Agent only knows: thread_id (as a key for checkpoints)
Agent does NOT own: threads, runs, messages, run_steps
```

## Agent Module Structure

```
agent/src/
├── config.rs, main.rs, service.rs     — Entry + gRPC service
├── common/                             — Zero-dependency domain types
│   ├── message.rs       Message, ContentBlock, Role
│   ├── token.rs         TokenUsage
│   ├── stream_event.rs  StreamEvent, StopReason (unified across providers)
│   ├── tool_types.rs    ToolOutput, ToolContext, ToolProgress, Artifact
│   └── checkpoint.rs    Checkpoint, ForkOrigin
├── infra/                              — Infrastructure layer
│   ├── store/                          ALL database/cache operations
│   │   ├── mod.rs         Store struct + fork logic
│   │   ├── pg/
│   │   │   ├── checkpoint.rs  PG checkpoint UPSERT/SELECT
│   │   │   ├── memory.rs      Memories CRUD + pgvector hybrid recall SQL
│   │   │   ├── tool_result.rs Large result persistence (>50KB → PG + preview)
│   │   │   └── audit.rs       Execution audit trail
│   │   └── redis/
│   │       ├── checkpoint.rs  Hot cache SET/GET (24h TTL)
│   │       └── event.rs       Redis Stream XADD for Gateway SSE
│   └── metrics/                        Observability
│       └── mod.rs         Atomic counters for turns, tools, tokens, errors
├── core/                               — Agent loop engine
│   ├── loop.rs          Main while(has_tool_use) loop
│   ├── context.rs       Context window pressure detection
│   ├── collapse.rs      L2 compression (no LLM, reversible)
│   ├── compact.rs       L3 compression (LLM summary, irreversible)
│   ├── error_recovery.rs Multi-layer recovery + exponential backoff
│   ├── hooks.rs         PreToolUse / PostToolUse / PreComplete hooks
│   ├── permission.rs    Auto / Ask / Deny per-tool permission
│   ├── plan_mode.rs     Planning mode (read-only tools)
│   └── xml.rs           <system-reminder> XML tag formatting
├── llm/                                — LLM provider abstraction
│   ├── provider.rs      LlmProvider trait, ProviderConfig, ToolDefinition
│   └── providers/       Anthropic SSE + OpenAI SSE implementations
├── tool/                               — Tool system
│   ├── definition.rs    Tool trait (types in common/tool_types.rs)
│   ├── registry.rs      Two-tier loading (resident + deferred)
│   ├── search.rs        ToolSearch built-in tool
│   └── executor.rs      Streaming concurrent executor + JSON Schema validation
├── skill/                              — Filesystem skills
│   ├── loader.rs        Scan data/skills/*/manifest.yaml, budget-controlled listing
│   ├── executor.rs      Inline / forked execution
│   └── discovery.rs     Intent-based skill recommendation
├── memory/                             — Memory types + injection logic
│   ├── types.rs         4 types: user, feedback, project, reference
│   ├── recall.rs        Hybrid retrieval (unconditional first)
│   └── inject.rs        Index → prompt, recalled → <system-reminder>
├── multi/                              — Sub-agent spawning
│   ├── agent_def.rs     YAML definitions (general, explorer, planner)
│   ├── spawn.rs         tokio::spawn with depth tracking
│   ├── isolation.rs     Context isolation + depth limit enforcement
│   └── fork.rs          Prompt cache sharing across fork children
├── mcp/                                — Model Context Protocol
│   ├── client.rs        MCP server connection (stdio/HTTP transport)
│   └── registry.rs      McpToolAdapter wraps MCP tools as our Tool trait
└── prompt/                             — System prompt construction
    ├── builder.rs       Section assembly with cache boundary
    ├── cache_boundary.rs Static/dynamic zone splitting
    ├── cache.rs         Section memoization across turns
    ├── instructions.rs  User/project level instructions
    └── rules.rs         Conditional rules (topic/tool match)
```

## Key Boundaries

```
common/ is the zero-dependency foundation — all modules import from it, it imports from nothing.

infra/store/ is the ONLY module that imports sqlx or redis.
All other modules talk to Store through clean Rust types from common/.

llm/ only contains provider abstraction (LlmProvider trait, ProviderConfig).
Domain types (Message, StreamEvent, etc.) live in common/, not llm/.

tool/definition.rs only contains the Tool trait.
Data types (ToolOutput, ToolContext, Artifact) live in common/tool_types.rs.

core/xml.rs is the ONLY place that produces XML tags.
memory/inject, skill/executor, multi/spawn, prompt/rules all import from core/xml.

prompt/builder.rs does NOT import from memory/.
It receives pre-formatted strings via PromptConfig.

Dependency direction: common → infra → llm/tool/memory/skill/mcp → core → service
Never reverse. No circular dependencies.
```

## Data Ownership

```
Agent owns (PG tables):
  checkpoints   — conversation snapshots (indexed by thread_id)
  memories      — persistent memory + pgvector
  tool_results  — large tool output persistence
  audit_log     — execution audit trail

Agent owns (Redis):
  checkpoint:{thread_id}  — hot cache, 24h TTL
  results:{thread_id}     — event stream for Gateway SSE

Agent owns (disk):
  data/skills/*/manifest.yaml + prompt.md

Agent does NOT own:
  threads, runs, messages, run_steps — Conversation module's tables
```

## Skill Format

```
data/skills/{name}/
├── manifest.yaml     # name, description, when_to_use, mode (inline/forked)
└── prompt.md         # full prompt, loaded on demand
```

## Rust Conventions

- Zero `unsafe` code
- No `unwrap()` — use `?` or `context()` for error propagation
- Every pub type/function has `///` doc comment
- File header comments explain module purpose + design decisions
- `pub(crate)` by default, `pub` only for cross-module API
- Errors include context: `"failed to connect to memory-service at {addr}"`
- Tests at bottom of each file in `#[cfg(test)] mod tests`

## TODO (Next Steps)

- [x] Move store/ and metrics/ under an `infra/` directory
- [x] Extract common/ module for shared domain types
- [x] Review module boundaries for clarity
- [ ] Fix compilation errors (IDE reports issues)
- [ ] Wire gRPC server in service.rs (tonic)
- [ ] Implement actual MCP protocol (currently stub)
- [ ] Add Prometheus exposition in metrics/
- [ ] Implement builtin tools (tool/builtin/)
- [ ] Wire permission checks into core/loop.rs
