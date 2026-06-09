pub mod store;
pub mod miner_key;
pub mod wallet;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FemConfig {
    pub miner_key: Option<String>,
    pub wallet_address: Option<String>,
    pub integrations_enabled: HashMap<String, bool>,
    pub api_base_url: String,
    #[serde(skip)]
    pub api_token: String,
}

impl Default for FemConfig {
    fn default() -> Self {
        Self {
            miner_key: None,
            wallet_address: None,
            integrations_enabled: HashMap::new(),
            api_base_url: "https://hardwareapi.frynetworks.com".to_string(),
            api_token: option_env!("FEM_API_TOKEN").unwrap_or("").to_string(),
        }
    }
}
