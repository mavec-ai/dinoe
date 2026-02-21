use crate::config::Config;
use crate::traits::Provider;
use crate::providers::{GlmProvider, OllamaProvider, OpenAIProvider, OpenRouterProvider};
use anyhow::{anyhow, Result};

pub fn create_provider(config: &Config) -> Result<Box<dyn Provider>> {
    let provider_name = config.provider.as_deref().unwrap_or("openai");

    match provider_name.to_lowercase().as_str() {
         "ollama" => {
            let mut provider = OllamaProvider::new();
            provider = provider.with_model(config.model.clone());
            if let Some(base_url) = &config.base_url {
                provider = provider.with_base_url(base_url.clone());
            }
            Ok(Box::new(provider))
        }
        "openai" => {
            let api_key = resolve_api_key_with_fallback(
                &["OPENAI_API_KEY", "DINOE_OPENAI_API_KEY"],
                &config.api_key,
            )?;
            let mut provider = OpenAIProvider::new(api_key);
            provider = provider.with_model(config.model.clone());
            if let Some(base_url) = &config.base_url {
                provider = provider.with_base_url(base_url.clone());
            }
            Ok(Box::new(provider))
        }
        "openrouter" => {
            let api_key = resolve_api_key_with_fallback(
                &["OPENROUTER_API_KEY", "DINOE_OPENROUTER_API_KEY"],
                &config.api_key,
            )?;
            let mut provider = OpenRouterProvider::new(api_key);
            provider = provider.with_model(config.model.clone());
            if let Some(base_url) = &config.base_url {
                provider = provider.with_base_url(base_url.clone());
            }
            Ok(Box::new(provider))
        }
        "zai" | "glm" => {
            let api_key = resolve_api_key_with_fallback(
                &["ZAI_API_KEY", "GLM_API_KEY", "DINOE_ZAI_API_KEY", "DINOE_GLM_API_KEY"],
                &config.api_key,
            )?;
            let mut provider = GlmProvider::new(api_key);
            provider = provider.with_model(config.model.clone());
            if let Some(base_url) = &config.base_url {
                provider = provider.with_base_url(base_url.clone());
            }
            Ok(Box::new(provider))
        }
        _ => Err(anyhow!("Unknown provider: {}. Available: openai, openrouter, ollama, glm/zai", provider_name)),
    }
}

fn resolve_api_key_with_fallback(env_vars: &[&str], config_key: &str) -> Result<String> {
    for var_name in env_vars {
        if let Ok(key) = resolve_api_key_from_env(var_name) {
            return Ok(key);
        }
    }
    if !config_key.is_empty() {
        Ok(config_key.to_string())
    } else {
        Err(anyhow!("No API key found"))
    }
}

fn resolve_api_key_from_env(var_name: &str) -> Result<String> {
    std::env::var(var_name).map_err(|_| anyhow!("Environment variable {} not set", var_name))
}
