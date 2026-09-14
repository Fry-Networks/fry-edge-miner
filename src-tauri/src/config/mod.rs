pub mod miner_key;
pub mod store;
pub mod wallet;

#[cfg(test)]
mod preservation_tests;
#[cfg(test)]
mod version_upgrade_tests;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FemConfig {
    pub miner_key: Option<String>,
    pub wallet_address: Option<String>,
    #[serde(default)]
    pub install_id: Option<String>,
    #[serde(default)]
    pub initial_setup_done: bool,
    pub integrations_enabled: HashMap<String, bool>,
    #[serde(default)]
    pub integration_versions: HashMap<String, String>,
    pub api_base_url: String,
    #[serde(skip_serializing, default = "default_api_token")]
    pub api_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    #[serde(default = "default_true")]
    pub start_on_boot: bool,
    #[serde(default = "default_true")]
    pub minimize_to_tray: bool,
    #[serde(default = "default_true")]
    pub auto_update: bool,
    #[serde(default = "default_true")]
    pub notifications: bool,
    #[serde(default)]
    pub myst_lan_override: bool,
    /// BUG 11/12: the app version this device last reported to the server
    /// via `attempt_version_change_heartbeat`. `#[serde(default)]` so a
    /// config saved by an older build (which never had this field) loads as
    /// `None`, which itself forces one immediate report on the next launch
    /// rather than silently staying stale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reported_version: Option<String>,
    /// BUG 1/2 (part d): the app version for which the one-time elevated
    /// Defender-exclusion + frynode-firewall hardening setup last completed
    /// successfully. `None`/a stale version means it should run again — once
    /// per version, not once per launch, so a declined UAC prompt does not
    /// re-nag on every single startup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardening_applied_version: Option<String>,
    /// BUG 1/4: root directory for partner binaries, installers and partner
    /// data (Space Acres plots, Iagon's node config). `None` — the only value
    /// any existing install has — means the historic
    /// `%APPDATA%\FryEdgeMiner\partners` location, so an upgrade is a
    /// byte-for-byte no-op: `skip_serializing_if` means the key is never even
    /// written until the user chooses a location.
    ///
    /// Stored as the RAW user-entered path; `storage_location::storage_root_for`
    /// appends the fixed `FryEdgeMiner\partners` leaf and
    /// `download::init_storage_root` validates it once at startup, falling back
    /// to the default on any problem.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_dir: Option<String>,
    /// B3: every key in fem_config.json this build does not recognise.
    ///
    /// `ConfigStore` saves by serializing this whole struct over the file, so
    /// without this catch-all anything serde skipped on load was destroyed the
    /// next time any setting changed. Named fields are matched first, so
    /// `api_token` (skip_serializing) is still consumed by its own field and
    /// can never be resurrected into the saved file through here.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl FemConfig {
    pub fn effective_api_token(&self) -> String {
        self.device_token
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.api_token.clone())
    }
}

fn default_true() -> bool {
    true
}

fn default_api_token() -> String {
    // Prefer runtime environment variable so the token is not baked into the binary.
    std::env::var("FEM_API_TOKEN")
        .ok()
        .or_else(|| option_env!("FEM_API_TOKEN").map(|s| s.to_string()))
        .unwrap_or_default()
}

impl Default for FemConfig {
    fn default() -> Self {
        Self {
            miner_key: None,
            wallet_address: None,
            install_id: None,
            initial_setup_done: false,
            integrations_enabled: HashMap::new(),
            integration_versions: HashMap::new(),
            api_base_url: "https://hardwareapi.frynetworks.com".to_string(),
            api_token: default_api_token(),
            device_token: None,
            device_name: None,
            start_on_boot: true,
            minimize_to_tray: true,
            auto_update: true,
            notifications: true,
            myst_lan_override: false,
            last_reported_version: None,
            hardening_applied_version: None,
            storage_dir: None,
            extra: serde_json::Map::new(),
        }
    }
}
