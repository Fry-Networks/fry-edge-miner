use std::path::PathBuf;
use std::sync::RwLock;

use anyhow::Result;

use crate::config::FemConfig;

/// Thread-safe configuration store backed by a JSON file
pub struct ConfigStore {
    config: RwLock<FemConfig>,
    path: PathBuf,
}

impl ConfigStore {
    /// Create a new ConfigStore, loading existing config from disk or using defaults
    pub fn new(config_dir: PathBuf) -> Result<Self> {
        let path = config_dir.join("fem_config.json");
        let config = if path.exists() {
            let data = std::fs::read_to_string(&path)?;
            serde_json::from_str(&data)?
        } else {
            FemConfig::default()
        };
        Ok(Self {
            config: RwLock::new(config),
            path,
        })
    }

    /// Get a clone of the current configuration
    pub fn get(&self) -> FemConfig {
        self.config.read().unwrap().clone()
    }

    /// Update the configuration using a closure and persist to disk
    pub fn update<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce(&mut FemConfig),
    {
        let mut config = self.config.write().unwrap();
        f(&mut config);
        self.save_to_disk(&config)
    }

    /// Persist current config to disk
    pub fn save(&self) -> Result<()> {
        let config = self.config.read().unwrap();
        self.save_to_disk(&config)
    }

    fn save_to_disk(&self, config: &FemConfig) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(config)?;
        std::fs::write(&self.path, data)?;
        Ok(())
    }
}
