use anyhow::{Context, Result};
use console::style;
use dialoguer::{Input, Select};
use dinoe_core::config::Config;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::templates::{DEFAULT_SOUL, DEFAULT_TOOLS, DEFAULT_USER};

const BANNER: &str = r"
    -------------------------------------

    ██████╗ ██╗███╗   ██╗ ██████╗ ███████╗
    ██╔══██╗██║████╗  ██║██╔═══██╗██╔════╝
    ██║  ██║██║██╔██╗ ██║██║   ██║█████╗  
    ██║  ██║██║██║╚██╗██║██║   ██║██╔══╝  
    ██████╔╝██║██║ ╚████║╚██████╔╝███████╗
    ╚═════╝ ╚═╝╚═╝  ╚═══╝ ╚═════╝ ╚══════╝

    -------------------------------------
";

fn print_step(step: usize, total: usize, title: &str) {
    println!();
    println!(
        "{}",
        style(format!("[{}/{}] {}", step, total, title))
            .cyan()
            .bold()
    );
    println!();
}

fn ensure_file(path: &Path, content: &str) -> Result<bool> {
    if !path.exists() {
        std::fs::write(path, content)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn create_bootstrap_files(workspace: &Path) -> Result<()> {
    std::fs::create_dir_all(workspace)?;

    ensure_file(&workspace.join("SOUL.md"), DEFAULT_SOUL)?;
    ensure_file(&workspace.join("TOOLS.md"), DEFAULT_TOOLS)?;
    ensure_file(&workspace.join("USER.md"), DEFAULT_USER)?;

    Ok(())
}

fn init_skills_dir(workspace: &Path) -> Result<()> {
    dinoe_core::skills::init_skills_dir(workspace)?;
    Ok(())
}

pub fn ensure_bootstrap_files(workspace: &Path) -> Result<()> {
    create_bootstrap_files(workspace)
}

fn setup_provider() -> Result<String> {
    let providers = [
        ("openai", "OpenAI"),
        ("openrouter", "OpenRouter"),
        ("ollama", "Ollama"),
        ("zai", "Z.AI (GLM)"),
    ];

    let provider_labels: Vec<&str> = providers.iter().map(|(_, label)| *label).collect();

    let selection = Select::new()
        .with_prompt("Select your AI provider")
        .items(&provider_labels)
        .default(0)
        .interact()
        .context("Failed to select provider")?;

    Ok(providers[selection].0.to_string())
}

fn setup_api_key(provider: &str) -> Result<String> {
    if provider == "ollama" {
        return Ok(String::new());
    }

    let prompt = match provider {
        "openrouter" => "Enter your OpenRouter API Key",
        "zai" => "Enter your Z.AI API Key",
        _ => "Enter your OpenAI API key",
    };

    let api_key: String = Input::new()
        .with_prompt(prompt)
        .interact_text()
        .context("Failed to read API key")?;

    if api_key.is_empty() {
        return Err(anyhow::anyhow!("API key cannot be empty"));
    }

    Ok(api_key)
}

const MODEL_CACHE_TTL_SECS: u64 = 12 * 60 * 60;
const MODEL_PREVIEW_LIMIT: usize = 20;
const CUSTOM_MODEL_SENTINEL: &str = "__custom__";

#[derive(Serialize, Deserialize)]
struct ModelCache {
    timestamp: u64,
    models: Vec<String>,
}

fn get_cache_path() -> std::path::PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("dinoe")
        .join("models_cache.json")
}

fn load_cached_models(provider: &str) -> Option<Vec<String>> {
    let cache_path = get_cache_path().join(format!("{}_models.json", provider));
    let content = std::fs::read_to_string(&cache_path).ok()?;
    let cache: ModelCache = serde_json::from_str(&content).ok()?;
    
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    
    if now - cache.timestamp < MODEL_CACHE_TTL_SECS {
        Some(cache.models)
    } else {
        None
    }
}

fn save_cached_models(provider: &str, models: &[String]) {
    let cache_dir = get_cache_path();
    if let Err(e) = std::fs::create_dir_all(&cache_dir) {
        eprintln!("{} Warning: Could not create cache dir: {}", style("!").yellow(), e);
        return;
    }
    
    let cache = ModelCache {
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        models: models.to_vec(),
    };
    
    let cache_path = cache_dir.join(format!("{}_models.json", provider));
    if let Err(e) = std::fs::write(&cache_path, serde_json::to_string_pretty(&cache).unwrap_or_default()) {
        eprintln!("{} Warning: Could not save cache: {}", style("!").yellow(), e);
    }
}

fn fetch_openrouter_models() -> Result<Vec<String>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("Failed to build HTTP client")?;
    
    let response = client
        .get("https://openrouter.ai/api/v1/models")
        .send()
        .context("Failed to fetch OpenRouter models")?;
    
    let json: serde_json::Value = response
        .json()
        .context("Failed to parse OpenRouter response")?;
    
    let mut models: Vec<String> = json
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(|id| id.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    
    models.sort();
    Ok(models)
}

fn fetch_ollama_models(base_url: &str) -> Result<Vec<String>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .context("Failed to build HTTP client")?;
    
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let response = client
        .get(&url)
        .send()
        .context("Failed to fetch Ollama models")?;
    
    let json: serde_json::Value = response
        .json()
        .context("Failed to parse Ollama response")?;
    
    let mut models: Vec<String> = json
        .get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    
    models.sort();
    Ok(models)
}

fn get_curated_models(provider: &str) -> Vec<String> {
    match provider {
        "openrouter" => vec![
            "anthropic/claude-sonnet-4".into(),
            "anthropic/claude-3.5-sonnet".into(),
            "openai/gpt-4o".into(),
            "google/gemini-2.0-flash-exp".into(),
            "meta-llama/llama-3.3-70b-instruct".into(),
        ],
        "ollama" => vec![
            "llama3.2".into(),
            "llama3.1".into(),
            "mistral".into(),
            "codellama".into(),
            "qwen2.5".into(),
        ],
        "zai" => vec!["glm-5".into(), "glm-4.7".into()],
        _ => vec![
            "gpt-5".into(),
            "gpt-5-mini".into(),
            "gpt-4o".into(),
            "gpt-4o-mini".into(),
        ],
    }
}

fn get_live_models(provider: &str, ollama_url: Option<&str>) -> Option<Vec<String>> {
    match provider {
        "openrouter" => {
            println!("{} Fetching models from OpenRouter...", style("→").cyan());
            match fetch_openrouter_models() {
                Ok(models) if !models.is_empty() => {
                    println!("{} Found {} models", style("✓").green(), models.len());
                    Some(models)
                }
                Ok(_) => {
                    println!("{} No models found, using defaults", style("!").yellow());
                    None
                }
                Err(e) => {
                    println!("{} Fetch failed: {}, using defaults", style("!").yellow(), e);
                    None
                }
            }
        }
        "ollama" => {
            let url = ollama_url.unwrap_or("http://localhost:11434");
            println!("{} Fetching models from Ollama ({})...", style("→").cyan(), url);
            match fetch_ollama_models(url) {
                Ok(models) if !models.is_empty() => {
                    println!("{} Found {} models", style("✓").green(), models.len());
                    Some(models)
                }
                Ok(_) => {
                    println!("{} No models found, using defaults", style("!").yellow());
                    None
                }
                Err(e) => {
                    println!("{} Fetch failed: {}, using defaults", style("!").yellow(), e);
                    None
                }
            }
        }
        _ => None,
    }
}

fn setup_model_with_ollama_url(provider: &str, ollama_url: Option<&str>) -> Result<String> {
    let cached = load_cached_models(provider);
    let mut models = if let Some(cached) = cached {
        println!("{} Using cached models ({} available)", style("✓").green(), cached.len());
        cached
    } else if let Some(live) = get_live_models(provider, ollama_url) {
        save_cached_models(provider, &live);
        live
    } else {
        get_curated_models(provider)
    };

    if models.len() > MODEL_PREVIEW_LIMIT {
        println!();
        println!("  {} Models (showing first {}):", style("-").dim(), MODEL_PREVIEW_LIMIT);
        for m in models.iter().take(MODEL_PREVIEW_LIMIT) {
            println!("    {} {}", style("-").dim(), m);
        }
        println!("    {} ... and {} more", style("-").dim(), models.len() - MODEL_PREVIEW_LIMIT);
    }

    models.push(CUSTOM_MODEL_SENTINEL.to_string());
    
    let selection = Select::new()
        .with_prompt("Select your model")
        .items(&models)
        .default(0)
        .interact()
        .context("Failed to select model")?;

    if models[selection] == CUSTOM_MODEL_SENTINEL {
        let custom: String = Input::new()
            .with_prompt("Enter model name")
            .interact_text()
            .context("Failed to read model name")?;
        Ok(custom)
    } else {
        Ok(models[selection].clone())
    }
}

fn setup_endpoint(provider: &str) -> Result<String> {
    match provider {
        "ollama" => {
            let default_url = "http://localhost:11434";
            let custom: bool = dialoguer::Confirm::new()
                .with_prompt("Use custom Ollama URL? (default: http://localhost:11434)")
                .default(false)
                .interact()
                .context("Failed to get custom URL preference")?;

            if custom {
                let url: String = Input::new()
                    .with_prompt("Enter Ollama base URL")
                    .default(default_url.to_string())
                    .interact_text()
                    .context("Failed to read URL")?;
                Ok(url)
            } else {
                Ok(default_url.to_string())
            }
        }
        "zai" => {
            let endpoints = [
                ("coding", "https://api.z.ai/api/coding/paas/v4 (GLM Coding)"),
                ("general", "https://api.z.ai/api/paas/v4"),
            ];

            let endpoint_labels: Vec<&str> = endpoints.iter().map(|(_, label)| *label).collect();

            let selection = Select::new()
                .with_prompt("Select Z.AI endpoint")
                .items(&endpoint_labels)
                .default(0)
                .interact()
                .context("Failed to select endpoint")?;

            Ok(endpoints[selection].0.to_string())
        }
        _ => Ok(String::new()),
    }
}

pub fn run_onboard() -> Result<Config> {
    println!("{}", style(BANNER).cyan().bold());

    println!("  {}", style("Welcome to Dinoe!").white().bold());
    println!(
        "  {}",
        style("This wizard will configure your agent in under 30 seconds.").dim()
    );
    println!();

    print_step(1, 5, "Provider Selection");
    let provider = setup_provider()?;

    print_step(2, 5, "API Key Setup");
    let api_key = setup_api_key(&provider)?;

    print_step(3, 5, "Endpoint Selection");
    let endpoint = setup_endpoint(&provider)?;
    let ollama_url = if provider == "ollama" {
        Some(if endpoint.is_empty() { "http://localhost:11434".to_string() } else { endpoint.clone() })
    } else {
        None
    };
    let base_url = if endpoint.is_empty() {
        match provider.as_str() {
            "openai" => Some("https://api.openai.com/v1".to_string()),
            "openrouter" => Some("https://openrouter.ai/api/v1".to_string()),
            _ => None,
        }
    } else {
        match provider.as_str() {
            "ollama" => Some(endpoint.clone()),
            "zai" => Some(match endpoint.as_str() {
                "coding" => "https://api.z.ai/api/coding/paas/v4".to_string(),
                "general" => "https://api.z.ai/api/paas/v4".to_string(),
                _ => String::new(),
            }),
            _ => Some(endpoint.clone()),
        }
    };

    print_step(4, 5, "Model Selection");
    let model = setup_model_with_ollama_url(&provider, ollama_url.as_deref())?;

    let config = Config {
        api_key,
        model,
        provider: Some(provider),
        base_url,
        ..Default::default()
    };

    print_step(5, 5, "Workspace Setup");
    if let Err(e) = create_bootstrap_files(&config.workspace_dir) {
        eprintln!(
            "  {} Warning: Could not create bootstrap files: {}",
            style("!").yellow(),
            e
        );
    } else {
        println!(
            "  {} Bootstrap files created at {}",
            style("✓").green(),
            style(config.workspace_dir.display()).cyan()
        );
        println!("  {} - SOUL.md", style("  ").dim());
        println!("  {} - TOOLS.md", style("  ").dim());
        println!("  {} - USER.md", style("  ").dim());
    }

    if let Err(e) = init_skills_dir(&config.workspace_dir) {
        eprintln!(
            "  {} Warning: Could not create skills directory: {}",
            style("!").yellow(),
            e
        );
    } else {
        println!(
            "  {} Skills directory ready at {}",
            style("✓").green(),
            style(config.workspace_dir.join("skills").display()).cyan()
        );
    }

    println!();
    println!("  {} Configuration complete!", style("✓").green().bold());
    println!(
        "  {} Config saved to {}",
        style("→").green(),
        style(dinoe_core::config::get_config_path().display()).cyan()
    );
    println!();
    println!(
        "  {} You can now run: {}",
        style("→").green(),
        style("dinoe chat").cyan().bold()
    );
    println!();

    Ok(config)
}
