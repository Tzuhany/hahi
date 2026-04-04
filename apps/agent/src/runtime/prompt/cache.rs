// ============================================================================
// Prompt Section Cache
//
// Static prompt sections (persona, tool schemas, skill listing, memory guide)
// don't change between turns. Rebuilding them every turn wastes CPU and
// breaks the LLM API's prompt cache (byte-identical prefixes get cached).
//
// This module memoizes prompt sections. A section is recomputed only when
// explicitly invalidated (e.g., after /clear or context compact).
//
// Claude Code does this with systemPromptSection() + a session-scoped cache.
// We do the same, but with an explicit PromptCache struct.
// ============================================================================

#![allow(dead_code)]

use std::collections::HashMap;

/// Caches prompt sections by label. Thread-safe via external Arc<Mutex<>>.
pub struct PromptCache {
    entries: HashMap<String, String>,
}

impl PromptCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Get a cached section, or compute and cache it.
    ///
    /// The `compute` closure is only called on cache miss.
    /// Sections with `None` result are NOT cached (they may become Some later).
    pub fn get_or_insert(
        &mut self,
        label: &str,
        compute: impl FnOnce() -> Option<String>,
    ) -> Option<String> {
        if let Some(cached) = self.entries.get(label) {
            return Some(cached.clone());
        }

        let value = compute()?;
        self.entries.insert(label.to_string(), value.clone());
        Some(value)
    }

    /// Invalidate all cached sections.
    ///
    /// Called when the prompt needs full rebuild:
    ///   - After context compact (prompt structure changed)
    ///   - After tool registration change
    ///   - On explicit clear
    pub fn clear(&mut self) {
        self.entries.clear();
        tracing::debug!("prompt cache cleared");
    }

    /// Invalidate a specific section.
    pub fn invalidate(&mut self, label: &str) {
        self.entries.remove(label);
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_miss_then_hit() {
        let mut cache = PromptCache::new();
        let mut calls = 0;

        let v1 = cache.get_or_insert("persona", || {
            calls += 1;
            Some("You are an AI.".into())
        });
        assert_eq!(v1.as_deref(), Some("You are an AI."));
        assert_eq!(calls, 1);

        // Second call → cache hit, closure not called.
        let v2 = cache.get_or_insert("persona", || {
            calls += 1;
            Some("SHOULD NOT SEE THIS".into())
        });
        assert_eq!(v2.as_deref(), Some("You are an AI."));
        assert_eq!(calls, 1); // Still 1 — closure was not called.
    }

    #[test]
    fn test_none_not_cached() {
        let mut cache = PromptCache::new();

        let v1 = cache.get_or_insert("empty", || None);
        assert!(v1.is_none());

        // Should recompute because None was not cached.
        let v2 = cache.get_or_insert("empty", || Some("now present".into()));
        assert_eq!(v2.as_deref(), Some("now present"));
    }

    #[test]
    fn test_clear() {
        let mut cache = PromptCache::new();
        cache.get_or_insert("a", || Some("value".into()));
        assert_eq!(cache.len(), 1);

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_invalidate_specific() {
        let mut cache = PromptCache::new();
        cache.get_or_insert("a", || Some("1".into()));
        cache.get_or_insert("b", || Some("2".into()));

        cache.invalidate("a");
        assert_eq!(cache.len(), 1);
    }
}
