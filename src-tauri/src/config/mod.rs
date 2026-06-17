pub mod store;
pub mod miner_key;
pub mod wallet;

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
    pub api_base_url: String,
    #[serde(skip_serializing, default = "default_api_token")]
    pub api_token: String,
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
            api_base_url: "https://hardwareapi.frynetworks.com".to_string(),
            api_token: default_api_token(),
        }
    }
}
