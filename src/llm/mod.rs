pub mod anthropic;
pub mod openai_compat;
pub mod provider;

use std::sync::Arc;

pub use provider::{ChatMessage, LlmError, LlmProvider};

/// Known provider presets with default base URLs.
struct ProviderPreset {
    base_url: &'static str,
    env_key: &'static str,
}

fn openai_compat_preset(provider: &str) -> Option<ProviderPreset> {
    match provider {
        "openai" => Some(ProviderPreset {
            base_url: "https://api.openai.com",
            env_key: "OPENAI_API_KEY",
        }),
        "deepseek" => Some(ProviderPreset {
            base_url: "https://api.deepseek.com",
            env_key: "DEEPSEEK_API_KEY",
        }),
        "ollama" => Some(ProviderPreset {
            base_url: "http://localhost:11434",
            env_key: "",
        }),
        "groq" => Some(ProviderPreset {
            base_url: "https://api.groq.com/openai",
            env_key: "GROQ_API_KEY",
        }),
        "together" => Some(ProviderPreset {
            base_url: "https://api.together.xyz",
            env_key: "TOGETHER_API_KEY",
        }),
        _ => None,
    }
}

/// Create an LLM provider from provider name and model.
///
/// Supported:
/// - OpenAI-compatible: "openai", "deepseek", "ollama", "groq", "together"
/// - Native: "anthropic"
/// - Custom: "custom" (reads LLM_BASE_URL + LLM_API_KEY)
pub fn create_provider(provider: &str, model: &str) -> Result<Arc<dyn LlmProvider>, LlmError> {
    if provider == "anthropic" {
        let api_key = env_or_err("ANTHROPIC_API_KEY", provider)?;
        return Ok(Arc::new(anthropic::AnthropicProvider::new(api_key, model)));
    }

    if provider == "custom" {
        let base_url = env_or_err("LLM_BASE_URL", provider)?;
        let api_key = std::env::var("LLM_API_KEY").unwrap_or_default();
        return Ok(Arc::new(openai_compat::OpenAiCompatProvider::new(base_url, api_key, model)));
    }

    if let Some(preset) = openai_compat_preset(provider) {
        let api_key = if preset.env_key.is_empty() {
            String::new()
        } else {
            env_or_err(preset.env_key, provider)?
        };
        return Ok(Arc::new(openai_compat::OpenAiCompatProvider::new(
            preset.base_url, api_key, model,
        )));
    }

    Err(LlmError::UnsupportedProvider { provider: provider.to_string() })
}

fn env_or_err(key: &str, provider: &str) -> Result<String, LlmError> {
    std::env::var(key).map_err(|_| LlmError::AuthFailed {
        provider: format!("{provider}: {key} not set"),
    })
}
