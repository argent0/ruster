use config::{Config as ConfigLoader, File};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::io::Write;
use anyhow::{Result, anyhow};
use home::home_dir;
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub socket_path: String,
    pub default_model: String,
    pub rag_model: String,
    pub skills_dirs: Vec<String>,
    pub initial_skills: Vec<String>,
    pub banned_skills: Vec<String>,
    pub proactive_interval_secs: u64,
    pub log_level: String,
    // Optional extra configuration not in spec but useful
    pub proxy_url: Option<String>,
}

impl Config {
    pub fn get_keys() -> Vec<String> {
        let val = serde_json::to_value(Config::default()).unwrap_or_default();
        if let Some(obj) = val.as_object() {
            obj.keys().cloned().collect()
        } else {
            Vec::new()
        }
    }

    pub fn get_value(&self, key: &str) -> Result<Value> {
        let val = serde_json::to_value(self)?;
        val.get(key)
            .cloned()
            .ok_or_else(|| anyhow!("Invalid configuration key: {}", key))
    }

    pub fn set_value(&mut self, key: &str, value: Value) -> Result<()> {
        let mut current_val = serde_json::to_value(self.clone())?;
        
        if let Some(map) = current_val.as_object_mut() {
            if !map.contains_key(key) {
                return Err(anyhow!("Invalid configuration key: {}", key));
            }
            map.insert(key.to_string(), value);
            
            let new_config: Config = serde_json::from_value(Value::Object(map.clone()))
                .map_err(|e| anyhow!("Invalid value for {}: {}", key, e))?;
            
            *self = new_config;
            self.save()?;
            Ok(())
        } else {
            Err(anyhow!("Internal error: Config is not an object"))
        }
    }

    pub fn save(&self) -> Result<()> {
        let config_dir = get_config_dir()?;
        let config_path = config_dir.join("config.toml");
        let toml_string = toml::to_string_pretty(self)
            .map_err(|e| anyhow!("Failed to serialize config: {}", e))?;
        
        let mut file = std::fs::File::create(&config_path)?;
        file.write_all(toml_string.as_bytes())?;
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            socket_path: "/tmp/ruster.sock".to_string(),
            default_model: "ollama/llama3.2".to_string(),
            rag_model: "ollama/nomic-embed-text".to_string(),
            skills_dirs: vec![
                "~/.config/ruster/skills".to_string(),
                "/usr/share/ruster/skills".to_string(),
            ],
            initial_skills: vec![],
            banned_skills: vec![],
            proactive_interval_secs: 300,
            log_level: "info".to_string(),
            proxy_url: Some("http://localhost:8080".to_string()),
        }
    }
}

pub fn load_config() -> Result<Config> {
    let config_dir = get_config_dir()?;
    std::fs::create_dir_all(&config_dir)?;

    let config_path = config_dir.join("config.toml");

    if !config_path.exists() {
        // Create default config
        let default_config = Config::default();
        let toml_string = toml::to_string_pretty(&default_config)
            .map_err(|e| anyhow::anyhow!("Failed to serialize default config: {}", e))?;
        
        let mut file = std::fs::File::create(&config_path)?;
        file.write_all(toml_string.as_bytes())?;
        tracing::info!("Created default config at {:?}", config_path);
    }

    let builder = ConfigLoader::builder()
        .add_source(File::from(config_path))
        .build()?;

    let config: Config = builder.try_deserialize()?;
    Ok(config)
}

pub fn get_config_dir() -> Result<PathBuf> {
    let home = home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    Ok(home.join(".config").join("ruster"))
}

pub fn expand_path(path: &str) -> PathBuf {
    if path.starts_with("~") {
        if let Some(home) = home_dir() {
            if path == "~" {
                return home;
            }
            if path.starts_with("~/") {
                return home.join(&path[2..]);
            }
        }
    }
    PathBuf::from(path)
}
