<div align="center">

<pre>
██╗  ██╗ █████╗ ██╗  ██╗██╗
██║  ██║██╔══██╗██║  ██║██║
███████║███████║███████║██║
██╔══██║██╔══██║██╔══██║██║
██║  ██║██║  ██║██║  ██║██║
╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝
</pre>

**A cloud-native AI agent runtime built on one principle:**
*the framework executes — the model thinks.*

![Rust](https://img.shields.io/badge/Rust-2024-orange?style=flat-square&logo=rust)
![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)
![Status](https://img.shields.io/badge/status-active_development-yellow?style=flat-square)

</div>

---

## What is Hahi?

Hahi is a general-purpose AI agent platform — not a coding assistant, not a chatbot wrapper. It can be DevOps automation, video generation orchestration, data analysis, or anything else. The domain is irrelevant. The runtime is domain-agnostic by design.

At its core, Hahi is a gRPC service that runs LLM + tool loops, streams results in real-time via Redis, and persists conversation state in PostgreSQL. It is built to run in the cloud, serve many concurrent users, and recover gracefully from anything that goes wrong.

---

## Design Philosophy

Five principles shape every decision in the codebase.

<br>

```
┌─────────────────────────────────────────────────────────────────┐
│  1. THE FRAMEWORK EXECUTES. THE MODEL THINKS.                   │
│                                                                 │
│  No ReAct loops. No chain-of-thought parsing. No framework-     │
│  level reasoning. The LLM decides what to do via native         │
│  tool_use. The framework executes tool calls and feeds          │
│  results back. That's it.                                       │
└─────────────────────────────────────────────────────────────────┘
```

```
┌─────────────────────────────────────────────────────────────────┐
│  2. TRUST THE MODEL.                                            │
│                                                                 │
│  Errors go back to the LLM as tool_result — not as exceptions.  │
│  The model decides how to recover. The framework only           │
│  intervenes for structural failures: context overflow,          │
│  rate limits, and provider errors.                              │
└─────────────────────────────────────────────────────────────────┘
```

```
┌─────────────────────────────────────────────────────────────────┐
│  3. INDEX ALWAYS PRESENT. CONTENT ON DEMAND.                    │
│                                                                 │
│                                                                 │
│  Tool schemas, skill descriptions, and memory entries are       │
│  lightweight summaries in the prompt. Full content loads only   │
│  when the LLM requests it. Saves ~60% of tool-related prompt    │
│  tokens without limiting capability.                            │
└─────────────────────────────────────────────────────────────────┘
```

```
┌─────────────────────────────────────────────────────────────────┐
│  4. THREE-LEVEL COMPRESSION.                                    │
│                                                                 │
│  L1 — Tool result budget     free, zero data loss               │
│  L2 — Context collapse        free, reversible                  │
│  L3 — LLM summarization       costs tokens, irreversible        │
│                                                                 │
│  The agent never fails because the context window is full.      │
│  It compresses and continues.                                   │
└─────────────────────────────────────────────────────────────────┘
```

```
┌─────────────────────────────────────────────────────────────────┐
│  5. SUB-AGENTS ARE TASKS, NOT PROCESSES.                        │
│                                                                 │
│  tokio::spawn — not child processes, not HTTP calls.            │
│  Sub-agents share LLM providers via Arc and run concurrently    │
│  in the same process. Depth-limited. Event-forwarded.           │
│  Isolated message histories.                                    │
└─────────────────────────────────────────────────────────────────┘
```

---

## Architecture

```
                                                     ┌─────────────────┐
  Client                                             │      Agent      │
  (browser / mobile / CLI)                           │                 │
      │                                              │  ┌───────────┐  │
      │  HTTP + SSE                                  │  │  kernel/  │  │
      ▼                                              │  │   loop    │  │
  ┌─────────┐                                        │  └─────┬─────┘  │
  │ Gateway │ ◄── Redis Stream (real-time events) ───┼────────┘        │
  └────┬────┘                                        │                 │
       │                                             │  PostgreSQL     │
       │  gRPC                                       │  Redis          │
       ▼                                             │  LLM API        │
  ┌──────────────────┐     gRPC                      │  Skills (disk)  │
  │ Conversation Mod │ ──────────────────────────────►                 │
  └──────────────────┘                               └─────────────────┘
  (owns Thread / Run /
   Message / RunStep)
```

**Ownership is explicit.** The Conversation Module owns conversation metadata. The Agent owns execution state: checkpoints, memories, tool results, audit logs. Neither reaches into the other's tables.

---

## Agent Internals

```
apps/agent/src/
│
├── common/          Zero-dependency domain types. The foundation.
│   │                Everything else imports from here. It imports nothing.
│   ├── message.rs       Message, ContentBlock, Role
│   ├── stream_event.rs  StreamEvent, StopReason
│   ├── tool_types.rs    ToolOutput, ToolContext, Artifact
│   └── checkpoint.rs    Checkpoint, PendingControl, ForkOrigin
│
├── kernel/          The loop engine. The beating heart.
│   ├── loop.rs          run_loop() — StreamProcessor — ToolDispatch
│   ├── compression/     L1 budget · L2 collapse · L3 compact
│   ├── hooks.rs         PreToolUse · PostToolUse · PreComplete
│   ├── permission.rs    Auto · Ask · Deny per-tool
│   ├── plan_mode.rs     Read-only planning before execution
│   ├── control.rs       Permission + plan review resume logic
│   └── event_bus.rs     MPMC event channel (loop → Redis → SSE)
│
├── systems/         Agent capabilities.
│   ├── memory/          Persistent memory + pgvector hybrid recall
│   ├── tools/           Two-tier registry · concurrent executor · MCP
│   ├── skills/          Filesystem skills (manifest.yaml + prompt.md)
│   └── subagents/       Spawn · isolation · depth limit · fork cache
│
├── adapters/        External world. Infrastructure boundary.
│   ├── llm/             LlmProvider trait · Anthropic SSE · OpenAI SSE
│   ├── store/           PostgreSQL + Redis (the ONLY place sqlx/redis live)
│   ├── metrics/         Prometheus-compatible atomic counters
│   ├── grpc/            tonic service adapter
│   └── mcp/             Model Context Protocol client
│
└── runtime/         Turn assembly. Wires everything together.
    ├── assembler.rs     RunPipeline::execute() — 14-step turn orchestration
    ├── builders.rs      Tool registry + system prompt construction
    └── prompt/          Section builder · cache boundary · memoization
```

**Dependency direction is one-way and enforced:**
```
common  ──►  adapters  ──►  systems  ──►  kernel  ──►  runtime
```
No module reaches backwards. No circular dependencies.

---

## Key Concepts

### The Loop

Every agent turn is a single while-loop. The LLM streams tokens. Tools execute in parallel as soon as their input is complete — not after the stream ends. Results accumulate. When the LLM stops calling tools, the turn finalizes.

```
LLM streams:  ──text──ToolA end──text──ToolB end──text──done──
ToolA:                 ├─────────────────────┤
ToolB:                              ├──────────────────┤
Results:                                                 ├──collected──►
```

### Two-Tier Tool Loading

```
Resident  →  Full schema in every prompt    (high-frequency: search, fetch)
Deferred  →  Name only, schema on demand    (low-frequency: email, cron, code)

Savings: ~60% fewer tool-prompt tokens. LLM discovers deferred tools via ToolSearch.
```

### Memory

Four typed memory categories — each with different recall behavior:

| Type | Contents | Recall |
|------|----------|--------|
| `user` | Who the user is, preferences, role | Always injected |
| `feedback` | Behavioral corrections from the user | Always injected |
| `project` | Ongoing work, deadlines, decisions | Semantic search |
| `reference` | Pointers to external systems | Semantic search |

Retrieval uses **hybrid RRF**: unconditional memories are always present; conditional memories are retrieved by pgvector similarity and ranked by recency + frequency.

### Plan Mode

When a task is complex, the agent can enter Plan Mode before acting:

```
Enter Plan Mode
     │
     ▼  (read-only tools only: search, fetch, query)
  Explore → Design → Submit plan
     │
     ▼
  User reviews
     │
  ┌──┴──────────────┐
  │                 │
Approve           Modify / Reject
  │                 │
  ▼                 ▼
Execute          Revise / End
```

### Sub-Agents

The main agent can spawn sub-agents for parallel or specialized work:

```
Parent Agent
├── Explorer  →  read-only research, returns findings
├── Planner   →  design-focused, returns structured plan
└── General   →  full tool access, returns output
```

Sub-agents run as `tokio::spawn` tasks. They share the LLM provider via `Arc`. Their events are forwarded to the parent's event stream (tool calls only — not text deltas). Depth is limited to prevent runaway recursion.

---

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Language | Rust (2024 edition) |
| Async runtime | Tokio |
| RPC | tonic (gRPC) |
| LLM streaming | Anthropic SSE, OpenAI SSE |
| Primary store | PostgreSQL + pgvector |
| Hot cache / events | Redis Streams |
| Tool protocol | MCP (Model Context Protocol) |
| Observability | tracing + Prometheus exposition |

---

## Getting Started

### Prerequisites

- Rust (stable, 2024 edition)
- PostgreSQL with pgvector extension
- Redis
- An Anthropic or OpenAI API key

### Configuration

```bash
cp .env.example .env
# Edit .env — set DATABASE_URL, REDIS_URL, ANTHROPIC_API_KEY
```

### Run

```bash
# Agent gRPC service
cargo run -p agent

# With logging
RUST_LOG=info cargo run -p agent
```

### Test

```bash
cargo test
```

---

## Project Layout

```
hahi/
├── apps/
│   ├── agent/      Execution runtime service
│   ├── gateway/    HTTP/SSE ingress service
│   └── session/    Conversation lifecycle service
├── contracts/      Proto sources + generated multi-language bindings
├── db/             Service migrations
├── deploy/         Docker and deployment assets
├── clients/        Web, mobile, and SDK consumers
└── data/           Skills filesystem (manifest.yaml + prompt.md)
```

---

<div align="center">

*Built with the conviction that the best agent framework*
*is the one that gets out of the model's way.*

</div>
