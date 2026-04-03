// ============================================================================
// Embedding Provider
//
// Thin async trait for generating text embeddings.
// Injected into memory tools so they can embed queries and content at write time.
//
// The default implementation (NoOpEmbedder) returns None — memory works in
// lexical-only mode. When an embedding provider is wired in, full RRF kicks in.
//
// Implementations live outside this crate (e.g., OpenAI text-embedding-3-small).
// ============================================================================

#![allow(dead_code)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Async function that produces a float vector for a text input.
pub type EmbedFuture = Pin<Box<dyn Future<Output = anyhow::Result<Vec<f32>>> + Send>>;

/// Async trait for embedding text into a float vector.
pub trait EmbeddingProvider: Send + Sync {
    /// Embed the given text. Returns None if embedding is unavailable.
    fn embed(&self, text: &str) -> EmbedFuture;

    /// Dimensionality of the embedding vectors produced by this provider.
    fn dimension(&self) -> usize;
}

/// Convenience alias used throughout the memory module.
pub type ArcEmbedder = Arc<dyn EmbeddingProvider>;

/// A no-op embedder that always returns an empty vec.
/// Used when no embedding provider is configured.
/// Memory falls back to lexical-only search.
pub struct NoOpEmbedder;

impl EmbeddingProvider for NoOpEmbedder {
    fn embed(&self, _text: &str) -> EmbedFuture {
        Box::pin(async { Ok(vec![]) })
    }

    fn dimension(&self) -> usize {
        0
    }
}

/// Embed text using the provider, returning None if the result is empty
/// (either because the provider is a no-op or because the call failed).
pub async fn try_embed(embedder: &ArcEmbedder, text: &str) -> Option<Vec<f32>> {
    match embedder.embed(text).await {
        Ok(v) if !v.is_empty() => Some(v),
        Ok(_) => None,
        Err(e) => {
            tracing::warn!(error = %e, "embedding failed, falling back to lexical-only");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_noop_embedder_returns_empty() {
        let e = Arc::new(NoOpEmbedder) as ArcEmbedder;
        let result = try_embed(&e, "hello world").await;
        assert!(result.is_none());
    }
}
