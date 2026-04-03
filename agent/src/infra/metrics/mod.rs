// ============================================================================
// Metrics — Agent Observability
//
// Prometheus-compatible metrics for monitoring the agent in production.
//
// Key metrics:
//   Counters:
//     agent_turns_total          — total turns processed
//     agent_tool_calls_total     — total tool invocations (by tool name)
//     agent_errors_total         — errors (by type: rate_limit, server, etc.)
//     agent_compact_total        — context compactions (by level: L1/L2/L3)
//     agent_tokens_total         — tokens consumed (by direction: input/output)
//
//   Histograms:
//     agent_turn_duration_seconds — turn latency distribution
//     agent_tool_duration_seconds — per-tool latency
//     agent_llm_duration_seconds  — LLM API call latency
//
//   Gauges:
//     agent_active_runs           — currently executing runs
//     agent_context_usage_ratio   — context window fill percentage
//
// Integration:
//   Metrics are recorded at key points in the agent loop (core/loop.rs),
//   tool executor (tool/executor.rs), and LLM providers (llm/providers/*.rs).
//   Exposed via /metrics HTTP endpoint for Prometheus scraping.
//
// For now: module structure and recording interface defined.
// Actual Prometheus integration (prometheus crate) added when needed.
// ============================================================================

use std::sync::atomic::{AtomicU64, Ordering};

/// Agent metrics collector. Shared via Arc across the agent.
pub struct Metrics {
    pub turns_total: AtomicU64,
    pub tool_calls_total: AtomicU64,
    pub errors_total: AtomicU64,
    pub compacts_total: AtomicU64,
    pub input_tokens_total: AtomicU64,
    pub output_tokens_total: AtomicU64,
    pub active_runs: AtomicU64,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            turns_total: AtomicU64::new(0),
            tool_calls_total: AtomicU64::new(0),
            errors_total: AtomicU64::new(0),
            compacts_total: AtomicU64::new(0),
            input_tokens_total: AtomicU64::new(0),
            output_tokens_total: AtomicU64::new(0),
            active_runs: AtomicU64::new(0),
        }
    }

    pub fn record_turn(&self) {
        self.turns_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_tool_call(&self) {
        self.tool_calls_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_error(&self) {
        self.errors_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Called by the L3 compaction path in core/compact.rs (not yet wired).
    #[allow(dead_code)]
    pub fn record_compact(&self) {
        self.compacts_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_tokens(&self, input: u64, output: u64) {
        self.input_tokens_total.fetch_add(input, Ordering::Relaxed);
        self.output_tokens_total
            .fetch_add(output, Ordering::Relaxed);
    }

    pub fn run_started(&self) {
        self.active_runs.fetch_add(1, Ordering::Relaxed);
    }

    pub fn run_ended(&self) {
        self.active_runs.fetch_sub(1, Ordering::Relaxed);
    }

    /// Format all metrics as a Prometheus text exposition.
    ///
    /// The output is suitable for scraping by Prometheus at the `/metrics` endpoint.
    pub fn to_prometheus(&self) -> String {
        let s = self.snapshot();
        format!(
            "# HELP agent_turns_total Total agent turns processed\n\
             # TYPE agent_turns_total counter\n\
             agent_turns_total {}\n\
             # HELP agent_tool_calls_total Total tool invocations\n\
             # TYPE agent_tool_calls_total counter\n\
             agent_tool_calls_total {}\n\
             # HELP agent_errors_total Total errors encountered\n\
             # TYPE agent_errors_total counter\n\
             agent_errors_total {}\n\
             # HELP agent_compacts_total Context compactions performed\n\
             # TYPE agent_compacts_total counter\n\
             agent_compacts_total {}\n\
             # HELP agent_input_tokens_total Input tokens consumed\n\
             # TYPE agent_input_tokens_total counter\n\
             agent_input_tokens_total {}\n\
             # HELP agent_output_tokens_total Output tokens generated\n\
             # TYPE agent_output_tokens_total counter\n\
             agent_output_tokens_total {}\n\
             # HELP agent_active_runs Currently executing runs\n\
             # TYPE agent_active_runs gauge\n\
             agent_active_runs {}\n",
            s.turns_total,
            s.tool_calls_total,
            s.errors_total,
            s.compacts_total,
            s.input_tokens_total,
            s.output_tokens_total,
            s.active_runs,
        )
    }

    /// Snapshot current values for reporting.
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            turns_total: self.turns_total.load(Ordering::Relaxed),
            tool_calls_total: self.tool_calls_total.load(Ordering::Relaxed),
            errors_total: self.errors_total.load(Ordering::Relaxed),
            compacts_total: self.compacts_total.load(Ordering::Relaxed),
            input_tokens_total: self.input_tokens_total.load(Ordering::Relaxed),
            output_tokens_total: self.output_tokens_total.load(Ordering::Relaxed),
            active_runs: self.active_runs.load(Ordering::Relaxed),
        }
    }
}

/// Point-in-time metrics snapshot.
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub turns_total: u64,
    pub tool_calls_total: u64,
    pub errors_total: u64,
    pub compacts_total: u64,
    pub input_tokens_total: u64,
    pub output_tokens_total: u64,
    pub active_runs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_recording() {
        let m = Metrics::new();
        m.record_turn();
        m.record_turn();
        m.record_tool_call();
        m.record_tokens(1000, 500);

        let snap = m.snapshot();
        assert_eq!(snap.turns_total, 2);
        assert_eq!(snap.tool_calls_total, 1);
        assert_eq!(snap.input_tokens_total, 1000);
        assert_eq!(snap.output_tokens_total, 500);
    }

    #[test]
    fn test_active_runs_gauge() {
        let m = Metrics::new();
        m.run_started();
        m.run_started();
        assert_eq!(m.snapshot().active_runs, 2);
        m.run_ended();
        assert_eq!(m.snapshot().active_runs, 1);
    }
}
