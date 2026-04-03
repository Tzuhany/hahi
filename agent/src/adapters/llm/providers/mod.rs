pub mod anthropic;
pub mod openai;

use crate::adapters::llm::LlmProvider;
use std::str::FromStr;
use std::sync::Arc;

/// Provider identifier, parsed from configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    Anthropic,
    OpenAI,
}

impl FromStr for ProviderKind {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "anthropic" => Ok(ProviderKind::Anthropic),
            "openai" => Ok(ProviderKind::OpenAI),
            other => Err(anyhow::anyhow!(
                "unsupported LLM_PROVIDER '{}'; expected 'anthropic' or 'openai'",
                other
            )),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_kind_from_env_value() {
        assert_eq!(
            "anthropic".parse::<ProviderKind>().unwrap(),
            ProviderKind::Anthropic
        );
        assert_eq!(
            "OPENAI".parse::<ProviderKind>().unwrap(),
            ProviderKind::OpenAI
        );
    }

    #[test]
    fn rejects_unknown_provider_kind() {
        let err = "other".parse::<ProviderKind>().unwrap_err();
        assert!(err.to_string().contains("unsupported LLM_PROVIDER"));
    }
}
