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
    /// Create a new ConfigStore, loading existing config from disk or using defaults.
    /// Logs diagnostics about config file state to help debug re-registration issues.
    pub fn new(config_dir: PathBuf) -> Result<Self> {
        let path = config_dir.join("fem_config.json");
        let backup_path = config_dir.join("fem_config.backup.json");

        tracing::info!(
            config_dir = %config_dir.display(),
            config_path = %path.display(),
            config_exists = path.exists(),
            "ConfigStore: resolving config file"
        );

        let config = if path.exists() {
            let data = std::fs::read_to_string(&path)?;
            let cfg: FemConfig = serde_json::from_str(&data)?;
            tracing::info!(
                has_miner_key = cfg.miner_key.is_some(),
                has_install_id = cfg.install_id.is_some(),
                has_wallet = cfg.wallet_address.is_some(),
                "ConfigStore: loaded existing config"
            );
            // Save a backup copy on successful load (for recovery after failed updates)
            if cfg.miner_key.is_some() {
                if let Err(e) = std::fs::copy(&path, &backup_path) {
                    tracing::warn!(error = %e, "ConfigStore: failed to save backup");
                }
            }
            cfg
        } else if backup_path.exists() {
            // Config missing but backup exists — recover from backup
            tracing::warn!("ConfigStore: config file missing, recovering from backup");
            let data = std::fs::read_to_string(&backup_path)?;
            let cfg: FemConfig = serde_json::from_str(&data)?;
            // Restore the main config file
            std::fs::copy(&backup_path, &path)?;
            tracing::info!(
                has_miner_key = cfg.miner_key.is_some(),
                "ConfigStore: recovered config from backup"
            );
            cfg
        } else {
            tracing::info!("ConfigStore: no config file found, using defaults (new install)");
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
