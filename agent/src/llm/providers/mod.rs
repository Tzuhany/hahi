pub mod anthropic;
pub mod openai;

use crate::llm::LlmProvider;
use std::sync::Arc;

/// Provider identifier, parsed from configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    Anthropic,
    #[allow(dead_code)]
    OpenAI,
}

/// Construct the appropriate LLM provider from configuration.
///
/// Returns an Arc because providers are shared across sub-agents.
/// Each sub-agent may use a different model, but the same provider instance.
///
/// # Errors
/// Returns Err if the API key environment variable is not set.
pub fn create_provider(kind: ProviderKind) -> anyhow::Result<Arc<dyn LlmProvider>> {
    match kind {
        ProviderKind::Anthropic => {
            let api_key = std::env::var("ANTHROPIC_API_KEY")
                .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY not set"))?;
            Ok(Arc::new(anthropic::AnthropicProvider::new(api_key)))
        }
        ProviderKind::OpenAI => {
            let api_key = std::env::var("OPENAI_API_KEY")
                .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY not set"))?;
            Ok(Arc::new(openai::OpenAIProvider::new(api_key)))
        }
    }
}
