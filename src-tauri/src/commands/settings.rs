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
    pub start_on_boot: bool,
    pub minimize_to_tray: bool,
    pub auto_update: bool,
    pub notifications: bool,
    pub config_warning: Option<String>,
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
            start_on_boot: cfg.start_on_boot,
            minimize_to_tray: cfg.minimize_to_tray,
            auto_update: cfg.auto_update,
            notifications: cfg.notifications,
            config_warning: None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct FemConfigUpdate {
    pub api_base_url: Option<String>,
    pub integrations_enabled: Option<HashMap<String, bool>>,
    pub start_on_boot: Option<bool>,
    pub minimize_to_tray: Option<bool>,
    pub auto_update: Option<bool>,
    pub notifications: Option<bool>,
}

#[tauri::command]
pub async fn get_settings(
    state: tauri::State<'_, crate::AppState>,
) -> Result<FemConfigView, String> {
    let mut view: FemConfigView = state.config.get().into();
    // B7: surface any load-time recovery/reset warning to the UI.
    view.config_warning = state.config.load_warning();
    Ok(view)
}

#[tauri::command]
pub async fn save_settings(
    app: tauri::AppHandle,
    settings: FemConfigUpdate,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    let start_on_boot_requested = settings.start_on_boot;
    state
        .config
        .update(|cfg| {
            if let Some(url) = settings.api_base_url {
                cfg.api_base_url = url;
            }
            if let Some(integrations) = settings.integrations_enabled {
                cfg.integrations_enabled = integrations;
            }
            if let Some(v) = settings.start_on_boot { cfg.start_on_boot = v; }
            if let Some(v) = settings.minimize_to_tray { cfg.minimize_to_tray = v; }
            if let Some(v) = settings.auto_update { cfg.auto_update = v; }
            if let Some(v) = settings.notifications { cfg.notifications = v; }
        })
        .map_err(|e| e.to_string())?;

    // B6: apply the autostart choice to the OS, not just the config file.
    if let Some(v) = start_on_boot_requested {
        use tauri_plugin_autostart::ManagerExt;
        let autolaunch = app.autolaunch();
        let result = if v {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        };
        match result {
            Ok(_) => tracing::info!(start_on_boot = v, "Autostart updated"),
            Err(e) => tracing::warn!(error = %e, "Autostart update failed"),
        }
    }

    tracing::info!("Settings saved");
    Ok(())
}
