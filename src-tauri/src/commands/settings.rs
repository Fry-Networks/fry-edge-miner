use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::config::FemConfig;

#[derive(Debug, Serialize)]
pub struct FemConfigView {
    pub miner_key: Option<String>,
    pub wallet_address: Option<String>,
    pub install_id: Option<String>,
    pub initial_setup_done: bool,
    pub integrations_enabled: HashMap<String, bool>,
    pub api_base_url: String,
}

impl From<FemConfig> for FemConfigView {
    fn from(cfg: FemConfig) -> Self {
        Self {
            miner_key: cfg.miner_key,
            wallet_address: cfg.wallet_address,
            install_id: cfg.install_id,
            initial_setup_done: cfg.initial_setup_done,
            integrations_enabled: cfg.integrations_enabled,
            api_base_url: cfg.api_base_url,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct FemConfigUpdate {
    pub api_base_url: Option<String>,
    pub integrations_enabled: Option<HashMap<String, bool>>,
}

#[tauri::command]
pub async fn get_settings(
    state: tauri::State<'_, crate::AppState>,
) -> Result<FemConfigView, String> {
    Ok(state.config.get().into())
}

#[tauri::command]
pub async fn save_settings(
    settings: FemConfigUpdate,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    state
        .config
        .update(|cfg| {
            if let Some(url) = settings.api_base_url {
                cfg.api_base_url = url;
            }
            if let Some(integrations) = settings.integrations_enabled {
                cfg.integrations_enabled = integrations;
            }
        })
        .map_err(|e| e.to_string())?;
    tracing::info!("Settings saved");
    Ok(())
}
