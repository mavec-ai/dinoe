use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DINOE_DIR: &str = ".dinoe";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StreamConfig {
    pub enabled: bool,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub provider: Option<String>,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
    pub max_iterations: usize,
    pub max_history: usize,
    pub temperature: f64,
    #[serde(default)]
    pub stream: StreamConfig,
    #[serde(skip)]
    pub workspace_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            provider: None,
            api_key: String::new(),
            base_url: None,
            model: "gpt-4o".to_string(),
            max_iterations: 20,
            max_history: 50,
            temperature: 1.0,
            stream: StreamConfig::default(),
            workspace_dir: get_dinoe_dir().join("workspace"),
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

impl Config {
    pub fn load_or_init() -> Result<Self> {
        if config_exists() {
            load_config()
        } else {
            Ok(Config::default())
        }
    }
}

pub fn load_config() -> Result<Config> {
    let config_path = get_config_path();

    let content = std::fs::read_to_string(&config_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!(
                "Config file not found. Run 'dinoe onboard' to set up your configuration."
            )
        } else {
            anyhow::anyhow!("Failed to read config from {}: {}", config_path.display(), e)
        }
    })?;

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
