// ============================================================================
// Prompt Cache Boundary
//
// The system prompt is split into two zones by a boundary marker:
//
//   ┌── Static Zone (identical across all users) ──┐
//   │  Base persona                                 │
//   │  Tool usage instructions                      │
//   │  Resident tool schemas                        │
//   │  Deferred tool name list                      │
//   │  Skill listing (1% budget)                    │
//   │  Memory behavioral instructions               │
//   ├── CACHE BOUNDARY ────────────────────────────┤
//   │  User-specific instructions                   │
//   │  Project-specific instructions                │
//   │  Memory index (≤200 entries)                  │
//   │  Conditional rules                            │
//   │  Date, environment context                    │
//   └── Dynamic Zone (per-user, per-session) ───────┘
//
// Anthropic's API supports prompt caching: identical prefixes across
// requests are cached and billed at ~90% discount.
//
// With 100k users, the static zone is identical for all of them.
// First request pays full price; subsequent requests hit cache.
// Savings: ~99% of input tokens for the static prefix.
//
// The boundary marker is stripped before sending to the API —
// it only exists to tell the cache logic where to split.
// ============================================================================

/// Marker that separates cacheable (static) from non-cacheable (dynamic) content.
///
/// Placed in the system prompt section array. The API layer uses this to
/// split the prompt into cache-eligible prefix and per-request suffix.
pub const CACHE_BOUNDARY: &str = "__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__";

/// A system prompt section with its cacheability.
#[derive(Debug, Clone)]
pub struct PromptSection {
    /// Human-readable label for debugging and logging.
    #[allow(dead_code)]
    pub label: &'static str,

    /// The content of this section. None means the section is disabled.
    pub content: Option<String>,
}

/// Join prompt sections into a single string for the API.
///
/// Sections are separated by double newlines. Empty/None sections are skipped.
/// The cache boundary marker is removed.
pub fn join_sections(sections: &[PromptSection]) -> String {
    sections
        .iter()
        .filter_map(|s| s.content.as_deref())
        .filter(|c| *c != CACHE_BOUNDARY)
        .collect::<Vec<&str>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_sections_skips_boundary() {
        let sections = vec![
            PromptSection {
                label: "a",
                content: Some("Part A".into()),
            },
            PromptSection {
                label: "b",
                content: Some(CACHE_BOUNDARY.into()),
            },
            PromptSection {
                label: "c",
                content: Some("Part C".into()),
            },
            PromptSection {
                label: "d",
                content: None,
            },
        ];

        let result = join_sections(&sections);
        assert_eq!(result, "Part A\n\nPart C");
    }
}
