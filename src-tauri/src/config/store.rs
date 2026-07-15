use std::path::PathBuf;
use std::sync::RwLock;

use anyhow::Result;

use crate::config::FemConfig;

/// Thread-safe configuration store backed by a JSON file.
///
/// B7 hardening: corrupt files are quarantined (never panic), every load
/// failure falls through Local backup → Roaming secondary → defaults with a
/// visible warning, and every save is atomic (tmp + rename) with redundant
/// copies so a wiped Local profile (Hyper-V enhanced-session, roaming resets)
/// no longer loses the miner key/wallet binding.
pub struct ConfigStore {
    config: RwLock<FemConfig>,
    path: PathBuf,
    backup_path: PathBuf,
    roaming_path: Option<PathBuf>,
    load_warning: RwLock<Option<String>>,
}

impl ConfigStore {
    pub fn new(config_dir: PathBuf) -> Self {
        let path = config_dir.join("fem_config.json");
        let backup_path = config_dir.join("fem_config.backup.json");
        let roaming_path =
            dirs::config_dir().map(|d| d.join("FryEdgeMiner").join("fem_config.json"));

        tracing::info!(
            config_dir = %config_dir.display(),
            config_exists = path.exists(),
            "ConfigStore: resolving config file"
        );

        let mut warning: Option<String> = None;
        let mut recovered_from: Option<&'static str> = None;

        let mut config = Self::try_load(&path, &mut warning, "primary");
        if config.is_none() {
            config = Self::try_load(&backup_path, &mut warning, "backup");
            if config.is_some() {
                recovered_from = Some("backup");
            }
        }
        if config.is_none() {
            if let Some(ref rp) = roaming_path {
                config = Self::try_load(rp, &mut warning, "roaming");
                if config.is_some() {
                    recovered_from = Some("roaming copy");
                }
            }
        }

        let had_any_file = path.exists()
            || backup_path.exists()
            || roaming_path.as_ref().map(|p| p.exists()).unwrap_or(false)
            || warning.is_some();

        let config = match config {
            Some(cfg) => {
                if let Some(src) = recovered_from {
                    warning = Some(format!(
                        "Settings were recovered from the {src} after the main config file was missing or unreadable. Verify your miner key and wallet in Settings."
                    ));
                    tracing::warn!(source = src, "ConfigStore: recovered config");
                }
                cfg
            }
            None => {
                if had_any_file {
                    warning = Some(
                        "Your saved settings could not be read and were reset. If this device was registered before, re-enter your existing miner key and wallet — do NOT register a new key. Corrupt files were kept next to the config for support."
                            .to_string(),
                    );
                    tracing::error!(
                        "ConfigStore: all config copies unreadable — starting with defaults"
                    );
                } else {
                    tracing::info!("ConfigStore: no config file found, using defaults (new install)");
                }
                FemConfig::default()
            }
        };

        tracing::info!(
            has_miner_key = config.miner_key.is_some(),
            has_install_id = config.install_id.is_some(),
            has_wallet = config.wallet_address.is_some(),
            "ConfigStore: loaded config"
        );

        let store = Self {
            config: RwLock::new(config),
            path,
            backup_path,
            roaming_path,
            load_warning: RwLock::new(warning),
        };
        // Re-persist immediately so recovered state propagates to every copy.
        if store.get().miner_key.is_some() {
            if let Err(e) = store.save() {
                tracing::warn!(error = %e, "ConfigStore: initial re-persist failed");
            }
        }
        store
    }

    /// Attempt to load one config file. Corrupt files are quarantined with a
    /// timestamped `.corrupt.` name so they can be inspected — never deleted.
    fn try_load(path: &PathBuf, warning: &mut Option<String>, label: &str) -> Option<FemConfig> {
        if !path.exists() {
            return None;
        }
        match std::fs::read_to_string(path) {
            Ok(data) => match serde_json::from_str::<FemConfig>(&data) {
                Ok(cfg) => Some(cfg),
                Err(e) => {
                    tracing::error!(file = %path.display(), error = %e, "ConfigStore: corrupt config ({label})");
                    let quarantine = path.with_file_name(format!(
                        "fem_config.corrupt.{}.json",
                        chrono::Utc::now().timestamp()
                    ));
                    if let Err(qe) = std::fs::rename(path, &quarantine) {
                        tracing::warn!(error = %qe, "ConfigStore: quarantine failed");
                    }
                    if warning.is_none() {
                        *warning = Some(format!(
                            "The {label} settings file was corrupt and has been quarantined ({}).",
                            quarantine.display()
                        ));
                    }
                    None
                }
            },
            Err(e) => {
                tracing::warn!(file = %path.display(), error = %e, "ConfigStore: read failed ({label})");
                None
            }
        }
    }

    /// Warning produced during load (corrupt/recovered/reset), for the UI.
    pub fn load_warning(&self) -> Option<String> {
        self.load_warning.read().unwrap().clone()
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
        let data = serde_json::to_string_pretty(config)?;
        Self::write_atomic(&self.path, &data)?;
        // Redundant copies only once a real registration exists.
        if config.miner_key.is_some() {
            if let Err(e) = Self::write_atomic(&self.backup_path, &data) {
                tracing::warn!(error = %e, "ConfigStore: backup write failed");
            }
            if let Some(ref rp) = self.roaming_path {
                if let Err(e) = Self::write_atomic(rp, &data) {
                    tracing::warn!(error = %e, "ConfigStore: roaming write failed");
                }
            }
        }
        Ok(())
    }

    /// Write via tmp-file + rename so a crash mid-write can never leave a
    /// truncated config behind.
    fn write_atomic(path: &PathBuf, data: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}
