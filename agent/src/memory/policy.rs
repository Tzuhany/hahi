// ============================================================================
// Memory Write Policy
//
// Validates WriteRequests before they reach the store.
// Two goals:
//   1. Defend against noise: empty content, oversized payloads
//   2. Normalize: trim whitespace, enforce name length
//
// Deliberately minimal — the LLM is trusted to decide what's worth remembering.
// Policy only enforces structural constraints, not semantic ones.
// ============================================================================

#![allow(dead_code)]

use crate::memory::types::{PINNED_KINDS, WriteRequest};

/// Max chars for pinned memory body (identity, feedback).
/// Pinned memories are injected every turn — keep them tight.
const PINNED_BODY_LIMIT: usize = 500;

/// Max chars for all other memory body.
const ARCHIVE_BODY_LIMIT: usize = 2_000;

/// Max chars for the memory name (shown in the index every turn).
const NAME_LIMIT: usize = 80;

/// Max chars for kind string.
const KIND_LIMIT: usize = 40;

/// A validated, normalized write request ready for the store.
#[derive(Debug, Clone)]
pub struct ValidatedWrite {
    pub agent_id: String,
    pub kind: String,
    pub name: String,
    pub body: String,
    pub ttl_days: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    EmptyKind,
    EmptyName,
    EmptyBody,
    /// Body exceeded the limit for this kind.
    /// The LLM should shorten and retry.
    BodyTooLong {
        kind: String,
        limit: usize,
        actual: usize,
    },
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyError::EmptyKind => write!(f, "kind must not be empty"),
            PolicyError::EmptyName => write!(f, "name must not be empty"),
            PolicyError::EmptyBody => write!(f, "body must not be empty"),
            PolicyError::BodyTooLong {
                kind,
                limit,
                actual,
            } => write!(
                f,
                "body too long for kind '{kind}': {actual} chars, limit is {limit}. \
                 Shorten and retry."
            ),
        }
    }
}

/// Validate and normalize a WriteRequest.
///
/// On success returns a ValidatedWrite ready for the store.
/// On failure returns a PolicyError whose Display is safe to return directly
/// to the LLM as a tool error (actionable, explains what to fix).
pub fn validate(req: WriteRequest) -> Result<ValidatedWrite, PolicyError> {
    // Normalize whitespace
    let kind = req.kind.trim().to_string();
    let name = req.name.trim().to_string();
    let body = req.body.trim().to_string();

    if kind.is_empty() {
        return Err(PolicyError::EmptyKind);
    }
    if name.is_empty() {
        return Err(PolicyError::EmptyName);
    }
    if body.is_empty() {
        return Err(PolicyError::EmptyBody);
    }

    // Silently truncate kind and name (they're metadata, not content).
    let kind = truncate_chars(&kind, KIND_LIMIT);
    let name = truncate_chars(&name, NAME_LIMIT);

    // Reject oversized body — tell the LLM to shorten.
    let body_limit = if PINNED_KINDS.contains(&kind.as_str()) {
        PINNED_BODY_LIMIT
    } else {
        ARCHIVE_BODY_LIMIT
    };

    let body_len = body.chars().count();
    if body_len > body_limit {
        return Err(PolicyError::BodyTooLong {
            kind,
            limit: body_limit,
            actual: body_len,
        });
    }

    Ok(ValidatedWrite {
        agent_id: req.agent_id,
        kind,
        name,
        body,
        ttl_days: req.ttl_days,
    })
}

fn truncate_chars(s: &str, limit: usize) -> String {
    if s.chars().count() <= limit {
        s.to_string()
    } else {
        s.chars().take(limit).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(kind: &str, name: &str, body: &str) -> WriteRequest {
        WriteRequest {
            agent_id: "agent-1".into(),
            kind: kind.into(),
            name: name.into(),
            body: body.into(),
            ttl_days: None,
        }
    }

    #[test]
    fn test_valid_request() {
        let result = validate(req("feedback", "no-emoji", "Don't add emoji."));
        assert!(result.is_ok());
        let v = result.unwrap();
        assert_eq!(v.kind, "feedback");
        assert_eq!(v.name, "no-emoji");
    }

    #[test]
    fn test_empty_kind_rejected() {
        assert_eq!(
            validate(req("", "name", "body")).unwrap_err(),
            PolicyError::EmptyKind
        );
    }

    #[test]
    fn test_empty_name_rejected() {
        assert_eq!(
            validate(req("feedback", "", "body")).unwrap_err(),
            PolicyError::EmptyName
        );
    }

    #[test]
    fn test_empty_body_rejected() {
        assert_eq!(
            validate(req("feedback", "name", "")).unwrap_err(),
            PolicyError::EmptyBody
        );
    }

    #[test]
    fn test_whitespace_only_rejected() {
        assert_eq!(
            validate(req("feedback", "  ", "body")).unwrap_err(),
            PolicyError::EmptyName
        );
        assert_eq!(
            validate(req("feedback", "name", "   ")).unwrap_err(),
            PolicyError::EmptyBody
        );
    }

    #[test]
    fn test_pinned_body_limit() {
        let long_body = "x".repeat(PINNED_BODY_LIMIT + 1);
        let err = validate(req("feedback", "name", &long_body)).unwrap_err();
        assert!(matches!(err, PolicyError::BodyTooLong { .. }));
    }

    #[test]
    fn test_archive_body_limit() {
        // Just under archive limit: ok
        let ok_body = "x".repeat(ARCHIVE_BODY_LIMIT);
        assert!(validate(req("experience", "name", &ok_body)).is_ok());

        // Over archive limit: rejected
        let long_body = "x".repeat(ARCHIVE_BODY_LIMIT + 1);
        assert!(matches!(
            validate(req("experience", "name", &long_body)).unwrap_err(),
            PolicyError::BodyTooLong { .. }
        ));
    }

    #[test]
    fn test_name_truncated_silently() {
        let long_name = "x".repeat(NAME_LIMIT + 50);
        let v = validate(req("experience", &long_name, "body")).unwrap();
        assert_eq!(v.name.chars().count(), NAME_LIMIT);
    }

    #[test]
    fn test_whitespace_trimmed() {
        let v = validate(req("  feedback  ", "  name  ", "  body  ")).unwrap();
        assert_eq!(v.kind, "feedback");
        assert_eq!(v.name, "name");
        assert_eq!(v.body, "body");
    }
}
