// ============================================================================
// Multi-Layer Error Recovery
//
// Errors are information, not exceptions. Most errors go back to the LLM,
// which decides how to recover. The framework only intervenes for structural
// failures the LLM can't see.
//
// Recovery layers:
//   1. Tool errors       → tool_result with is_error=true → LLM handles
//   2. Max output tokens → escalate → inject continuation → abort
//   3. Context overflow   → compact → abort
//   4. Rate limit        → exponential backoff with jitter
//   5. Server error (5xx) → exponential backoff (fewer retries)
//   6. Diminishing returns → auto-stop (budget-aware)
// ============================================================================

use std::time::Duration;

use crate::common::StopReason;

/// Minimum useful output per turn. Below this = "not making progress."
const MIN_USEFUL_TOKENS: u64 = 500;

/// Consecutive low-output turns before auto-stop (normal mode).
const MAX_LOW_OUTPUT_TURNS: u32 = 3;

/// Max output token escalation attempts.
const MAX_TOKEN_ESCALATIONS: u32 = 2;

/// Default and escalated max output tokens.
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 16_384;
const ESCALATED_MAX_OUTPUT_TOKENS: u32 = 65_536;

/// Exponential backoff configuration.
const BACKOFF_BASE_MS: u64 = 500;
const BACKOFF_MAX_MS: u64 = 30_000;
const MAX_RATE_LIMIT_RETRIES: u32 = 10;
const MAX_SERVER_ERROR_RETRIES: u32 = 3;

/// Recovery state, carried through the loop.
#[derive(Debug, Clone)]
pub struct RecoveryState {
    pub token_escalation_count: u32,
    pub low_output_streak: u32,
    pub has_compacted: bool,
    pub rate_limit_retries: u32,
    pub server_error_retries: u32,

    /// If true, diminishing returns detection is disabled.
    /// Used when the user explicitly wants to spend tokens (budget mode).
    pub budget_mode: bool,
}

impl Default for RecoveryState {
    fn default() -> Self {
        Self {
            token_escalation_count: 0,
            low_output_streak: 0,
            has_compacted: false,
            rate_limit_retries: 0,
            server_error_retries: 0,
            budget_mode: false,
        }
    }
}

/// What the loop should do next.
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryAction {
    Continue,
    EscalateTokens { new_max_tokens: u32 },
    InjectContinuation,
    CompactAndRetry,
    WaitAndRetry { duration: Duration },
    StopDiminishingReturns,
    Abort { reason: String },
}

/// High-level loop control decision, derived from a RecoveryAction.
///
/// Side-effect actions (inject continuation, compact, sleep) are handled by
/// the caller before calling `into_loop_control`. This type carries only the
/// pure control-flow outcome.
#[derive(Debug, Clone, PartialEq)]
pub enum LoopControl {
    /// Continue to the next iteration normally.
    Continue,
    /// Retry the API call this iteration.
    Retry,
    /// Return from the loop with the given stop reason.
    Return(crate::kernel::r#loop::TurnStopReason),
}

impl RecoveryAction {
    /// Map to a pure control-flow decision.
    ///
    /// Side-effect variants (InjectContinuation, CompactAndRetry, WaitAndRetry)
    /// map to `LoopControl::Retry` — the caller is responsible for performing
    /// the side effect before reaching this call.
    pub fn into_loop_control(self) -> LoopControl {
        match self {
            RecoveryAction::Continue => LoopControl::Continue,
            RecoveryAction::EscalateTokens { .. }
            | RecoveryAction::InjectContinuation
            | RecoveryAction::CompactAndRetry
            | RecoveryAction::WaitAndRetry { .. } => LoopControl::Retry,
            RecoveryAction::StopDiminishingReturns => {
                LoopControl::Return(crate::kernel::r#loop::TurnStopReason::DiminishingReturns)
            }
            RecoveryAction::Abort { reason } => {
                LoopControl::Return(crate::kernel::r#loop::TurnStopReason::Error(reason))
            }
        }
    }
}

/// Classify an API error for recovery routing.
pub enum ApiErrorKind {
    /// 429 Too Many Requests — rate limited.
    RateLimit { retry_after_seconds: Option<u32> },
    /// 5xx — server error, transient.
    ServerError,
    /// 413 — prompt too long.
    PromptTooLong,
    /// Other — not recoverable.
    Other { message: String },
}

/// Evaluate the LLM response and determine recovery action.
pub fn evaluate(
    stop_reason: &StopReason,
    output_tokens: u64,
    state: &mut RecoveryState,
    api_error: Option<&ApiErrorKind>,
) -> RecoveryAction {
    // API errors take priority.
    if let Some(error) = api_error {
        return evaluate_api_error(error, state);
    }

    // Max tokens hit → escalate or inject continuation.
    if *stop_reason == StopReason::MaxTokens {
        state.token_escalation_count += 1;

        if state.token_escalation_count == 1 {
            return RecoveryAction::EscalateTokens {
                new_max_tokens: ESCALATED_MAX_OUTPUT_TOKENS,
            };
        }
        if state.token_escalation_count <= MAX_TOKEN_ESCALATIONS {
            return RecoveryAction::InjectContinuation;
        }
        return RecoveryAction::Abort {
            reason: "output truncated after multiple escalations".into(),
        };
    }

    // Diminishing returns detection (skip in budget mode).
    if !state.budget_mode {
        if output_tokens < MIN_USEFUL_TOKENS {
            state.low_output_streak += 1;
            if state.low_output_streak >= MAX_LOW_OUTPUT_TURNS {
                return RecoveryAction::StopDiminishingReturns;
            }
        } else {
            state.low_output_streak = 0;
        }
    }

    RecoveryAction::Continue
}

/// Handle API-level errors with appropriate retry strategy.
fn evaluate_api_error(error: &ApiErrorKind, state: &mut RecoveryState) -> RecoveryAction {
    match error {
        ApiErrorKind::RateLimit {
            retry_after_seconds,
        } => {
            state.rate_limit_retries += 1;
            if state.rate_limit_retries > MAX_RATE_LIMIT_RETRIES {
                return RecoveryAction::Abort {
                    reason: format!("rate limited {} times, giving up", MAX_RATE_LIMIT_RETRIES),
                };
            }

            let duration = match retry_after_seconds {
                Some(s) => Duration::from_secs(*s as u64),
                None => backoff_duration(state.rate_limit_retries),
            };

            tracing::warn!(
                retry = state.rate_limit_retries,
                wait_ms = duration.as_millis() as u64,
                "rate limited, backing off"
            );

            RecoveryAction::WaitAndRetry { duration }
        }

        ApiErrorKind::ServerError => {
            state.server_error_retries += 1;
            if state.server_error_retries > MAX_SERVER_ERROR_RETRIES {
                return RecoveryAction::Abort {
                    reason: format!("server error {} times, giving up", MAX_SERVER_ERROR_RETRIES),
                };
            }

            let duration = backoff_duration(state.server_error_retries);
            tracing::warn!(
                retry = state.server_error_retries,
                wait_ms = duration.as_millis() as u64,
                "server error, backing off"
            );

            RecoveryAction::WaitAndRetry { duration }
        }

        ApiErrorKind::PromptTooLong => {
            if state.has_compacted {
                return RecoveryAction::Abort {
                    reason: "prompt too long after compaction".into(),
                };
            }
            state.has_compacted = true;
            RecoveryAction::CompactAndRetry
        }

        ApiErrorKind::Other { message } => RecoveryAction::Abort {
            reason: message.clone(),
        },
    }
}

/// Exponential backoff with jitter.
///
/// delay = min(base × 2^(attempt-1) + jitter, max)
/// jitter = random [0, base) — prevents thundering herd when multiple callers
/// hit the same rate-limit window and retry simultaneously.
fn backoff_duration(attempt: u32) -> Duration {
    use rand::Rng;
    let exp = BACKOFF_BASE_MS.saturating_mul(1u64 << (attempt - 1).min(10));
    let jitter = rand::thread_rng().gen_range(0..BACKOFF_BASE_MS);
    let total = (exp + jitter).min(BACKOFF_MAX_MS);
    Duration::from_millis(total)
}

/// The continuation prompt injected when output was truncated.
pub fn continuation_prompt() -> &'static str {
    "Your previous response was truncated due to length limits. \
     Continue exactly where you left off. Do not repeat what you already said."
}

/// Determine max_tokens for the current API call.
pub fn effective_max_tokens(state: &RecoveryState) -> u32 {
    if state.token_escalation_count > 0 {
        ESCALATED_MAX_OUTPUT_TOKENS
    } else {
        DEFAULT_MAX_OUTPUT_TOKENS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_response_continues() {
        let mut state = RecoveryState::default();
        let action = evaluate(&StopReason::EndTurn, 1000, &mut state, None);
        assert_eq!(action, RecoveryAction::Continue);
    }

    #[test]
    fn test_max_tokens_escalates() {
        let mut state = RecoveryState::default();
        let action = evaluate(&StopReason::MaxTokens, 16000, &mut state, None);
        assert!(matches!(action, RecoveryAction::EscalateTokens { .. }));
    }

    #[test]
    fn test_rate_limit_exponential_backoff() {
        let mut state = RecoveryState::default();

        let err = ApiErrorKind::RateLimit {
            retry_after_seconds: None,
        };
        let a1 = evaluate(&StopReason::EndTurn, 0, &mut state, Some(&err));
        let a2 = evaluate(&StopReason::EndTurn, 0, &mut state, Some(&err));

        // Second retry should wait longer.
        match (a1, a2) {
            (
                RecoveryAction::WaitAndRetry { duration: d1 },
                RecoveryAction::WaitAndRetry { duration: d2 },
            ) => {
                assert!(d2 > d1, "backoff should increase: {:?} vs {:?}", d1, d2);
            }
            _ => panic!("expected WaitAndRetry"),
        }
    }

    #[test]
    fn test_rate_limit_respects_retry_after() {
        let mut state = RecoveryState::default();
        let err = ApiErrorKind::RateLimit {
            retry_after_seconds: Some(30),
        };
        let action = evaluate(&StopReason::EndTurn, 0, &mut state, Some(&err));
        assert_eq!(
            action,
            RecoveryAction::WaitAndRetry {
                duration: Duration::from_secs(30)
            }
        );
    }

    #[test]
    fn test_rate_limit_gives_up_after_max() {
        let mut state = RecoveryState {
            rate_limit_retries: MAX_RATE_LIMIT_RETRIES,
            ..Default::default()
        };
        let err = ApiErrorKind::RateLimit {
            retry_after_seconds: None,
        };
        let action = evaluate(&StopReason::EndTurn, 0, &mut state, Some(&err));
        assert!(matches!(action, RecoveryAction::Abort { .. }));
    }

    #[test]
    fn test_diminishing_returns_skipped_in_budget_mode() {
        let mut state = RecoveryState {
            budget_mode: true,
            ..Default::default()
        };

        // 3 consecutive low-output turns.
        evaluate(&StopReason::EndTurn, 100, &mut state, None);
        evaluate(&StopReason::EndTurn, 100, &mut state, None);
        let action = evaluate(&StopReason::EndTurn, 100, &mut state, None);

        // Budget mode → should NOT stop.
        assert_eq!(action, RecoveryAction::Continue);
    }

    #[test]
    fn test_diminishing_returns_fires_in_normal_mode() {
        let mut state = RecoveryState::default();
        evaluate(&StopReason::EndTurn, 100, &mut state, None);
        evaluate(&StopReason::EndTurn, 100, &mut state, None);
        let action = evaluate(&StopReason::EndTurn, 100, &mut state, None);
        assert_eq!(action, RecoveryAction::StopDiminishingReturns);
    }

    #[test]
    fn test_backoff_increases() {
        let d1 = backoff_duration(1);
        let d2 = backoff_duration(2);
        let d3 = backoff_duration(3);
        assert!(d2 > d1);
        assert!(d3 > d2);
    }

    #[test]
    fn test_backoff_capped() {
        let d = backoff_duration(20); // Very high attempt.
        assert!(d.as_millis() <= BACKOFF_MAX_MS as u128);
    }
}
