use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DINOE_DIR: &str = ".dinoe";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub api_key: String,
    pub model: String,
    pub base_url: Option<String>,
    #[serde(skip)]
    pub workspace_dir: PathBuf,
    pub max_iterations: usize,
    pub max_history: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            api_key: String::new(),
            model: "gpt-4o".to_string(),
            base_url: None,
            workspace_dir: get_dinoe_dir().join("workspace"),
            max_iterations: 20,
            max_history: 50,
        }
    }
}

pub fn get_dinoe_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(DINOE_DIR)
}

pub fn get_config_path() -> PathBuf {
    get_dinoe_dir().join("config.toml")
}

pub fn ensure_dinoe_dir() -> Result<PathBuf> {
    let dinoe_dir = get_dinoe_dir();

    if !dinoe_dir.exists() {
        std::fs::create_dir_all(&dinoe_dir).with_context(|| {
            format!(
                "Failed to create dinoe directory at {}",
                dinoe_dir.display()
            )
        })?;
    }

    Ok(dinoe_dir)
}

pub fn load_config() -> Result<Config> {
    let config_path = get_config_path();

    if !config_path.exists() {
        return Err(anyhow::anyhow!(
            "Config file not found. Run 'dinoe onboard' to set up your configuration."
        ));
    }

    let content = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config from {}", config_path.display()))?;

    let mut config: Config = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config from {}", config_path.display()))?;

    config.workspace_dir = get_dinoe_dir().join("workspace");

    Ok(config)
}

pub fn save_config(config: &Config) -> Result<()> {
    ensure_dinoe_dir()?;

    let config_path = get_config_path();
    let content =
        toml::to_string_pretty(config).with_context(|| "Failed to serialize config to TOML")?;

    std::fs::write(&config_path, content)
        .with_context(|| format!("Failed to write config to {}", config_path.display()))?;

    Ok(())
}

pub fn config_exists() -> bool {
    get_config_path().exists()
}
