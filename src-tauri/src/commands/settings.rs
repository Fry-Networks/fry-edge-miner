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
    /// BUG 1/4: the configured override (None = the historic %APPDATA% location).
    pub storage_dir: Option<String>,
    /// BUG 1/4: the root actually in use THIS session. Differs from
    /// `storage_dir` until the app is restarted, which is how the UI knows to
    /// show "restart to use this location".
    pub storage_dir_active: String,
    /// Whether the scrubbed debug-log sink is currently writing.
    pub debug_logging_enabled: bool,
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
            storage_dir: cfg.storage_dir,
            storage_dir_active: crate::integrations::download::partners_base_dir()
                .to_string_lossy()
                .into_owned(),
            debug_logging_enabled: cfg.debug_logging_enabled,
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
            if let Some(v) = settings.start_on_boot {
                cfg.start_on_boot = v;
            }
            if let Some(v) = settings.minimize_to_tray {
                cfg.minimize_to_tray = v;
            }
            if let Some(v) = settings.auto_update {
                cfg.auto_update = v;
            }
            if let Some(v) = settings.notifications {
                cfg.notifications = v;
            }
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

/// BUG 2 (1337): "No way to change the wallet address once entered."
///
/// The Settings page showed the address through a read-only `CopyField` and the
/// input only existed in the un-registered branch, so a typo at setup was
/// permanent. There is deliberately NO backend endpoint here: FEM never
/// transmitted this value (`InstallationHeartbeat` has no wallet field), and
/// payout authority lives in `main.devices.reward_wallet` on the dashboard,
/// which is session-gated. A device-token-authenticated write would let anyone
/// holding a miner key redirect another user's payouts, so the UI links to the
/// dashboard for that and this command only fixes the local value.
#[tauri::command]
pub async fn set_wallet_address(
    address: String,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    let trimmed = address.trim().to_uppercase();
    crate::config::wallet::validate_address(&trimmed).map_err(|e| e.to_string())?;
    state
        .config
        .update(|cfg| cfg.wallet_address = Some(trimmed.clone()))
        .map_err(|e| e.to_string())?;
    tracing::info!("Wallet address updated locally");
    Ok(())
}

/// BUG 1/4: where partner binaries and partner data live.
#[derive(Debug, serde::Serialize)]
pub struct StorageLocation {
    /// The root FEM will use (already including the FryEdgeMiner\partners leaf).
    pub path: String,
    /// Free space on that volume, or None if it could not be measured.
    pub free_gb: Option<f64>,
    pub is_default: bool,
    /// Configured != active, i.e. a restart is needed for it to take effect.
    pub pending_restart: bool,
    /// B4: where partner files are being written RIGHT NOW. Equal to `path`
    /// unless the configured root was rejected at startup, in which case the
    /// two disagree and this is the one that is true.
    pub active_path: String,
    /// B4: why the configured root was not used, if it was not. When this is
    /// set, `pending_restart` is true but a restart alone will NOT help, so the
    /// UI must show this instead of the restart banner.
    pub fallback_reason: Option<String>,
}

fn describe_storage(configured: Option<&str>) -> StorageLocation {
    let default = crate::integrations::download::default_partners_base_dir();
    let active = crate::integrations::download::partners_base_dir();
    let resolved =
        crate::integrations::download::resolve_partners_base_dir(configured, default.clone());
    StorageLocation {
        free_gb: crate::system_info::available_disk_gb(&active),
        is_default: resolved == default,
        pending_restart: resolved != active,
        path: resolved.to_string_lossy().into_owned(),
        active_path: active.to_string_lossy().into_owned(),
        fallback_reason: crate::integrations::download::storage_fallback_reason(),
    }
}

#[tauri::command]
pub async fn get_storage_location(
    state: tauri::State<'_, crate::AppState>,
) -> Result<StorageLocation, String> {
    Ok(describe_storage(state.config.get().storage_dir.as_deref()))
}

/// `path: None` or an empty string reverts to the historic default.
#[tauri::command]
pub async fn set_storage_location(
    path: Option<String>,
    state: tauri::State<'_, crate::AppState>,
) -> Result<StorageLocation, String> {
    let cleaned = path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());

    if let Some(ref p) = cleaned {
        let install_dir = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.to_path_buf()))
            .unwrap_or_default();
        crate::storage_location::validate_pure(p, &install_dir).map_err(|e| e.message())?;
        let root = crate::storage_location::storage_root_for(p);
        crate::storage_location::probe_writable(&root).map_err(|e| e.message())?;
    }

    state
        .config
        .update(|cfg| cfg.storage_dir = cleaned.clone())
        .map_err(|e| e.to_string())?;
    tracing::info!(is_default = cleaned.is_none(), "Storage location updated");
    Ok(describe_storage(cleaned.as_deref()))
}
