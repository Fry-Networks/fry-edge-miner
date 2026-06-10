use rand::rngs::OsRng;
use rand::RngCore;
use serde::Serialize;

use crate::api::types::InstallationHeartbeat;

#[derive(Debug, Serialize)]
pub struct DeviceInfo {
    pub miner_key: Option<String>,
    pub wallet_address: Option<String>,
    pub registered: bool,
}

fn generate_install_id() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

#[tauri::command]
pub async fn get_device_info(
    state: tauri::State<'_, crate::AppState>,
) -> Result<DeviceInfo, String> {
    let config = state.config.get();
    Ok(DeviceInfo {
        registered: config.miner_key.is_some(),
        miner_key: config.miner_key,
        wallet_address: config.wallet_address,
    })
}

#[tauri::command]
pub async fn register_device(
    wallet: String,
    state: tauri::State<'_, crate::AppState>,
) -> Result<String, String> {
    crate::config::wallet::validate_address(&wallet).map_err(|e| e.to_string())?;

    let miner_key = crate::config::miner_key::generate();
    let install_id = generate_install_id();

    // Save miner_key + wallet to config first (install_id saved after API success)
    state
        .config
        .update(|cfg| {
            cfg.miner_key = Some(miner_key.clone());
            cfg.wallet_address = Some(wallet.clone());
        })
        .map_err(|e| e.to_string())?;

    // Register with the hardwareapi
    let heartbeat = InstallationHeartbeat {
        miner_key: miner_key.clone(),
        install_id: install_id.clone(),
        miner_code: Some("FEM".to_string()),
        software_version_installed: Some(env!("CARGO_PKG_VERSION").to_string()),
        poc_version_installed: Some("1.0.0".to_string()),
        hostname: std::env::var("COMPUTERNAME")
            .ok()
            .or_else(|| std::env::var("HOSTNAME").ok()),
        os: Some(std::env::consts::OS.to_string()),
        is_installed: Some(true),
    };

    match crate::api::installations::register(&state.api_client, &heartbeat).await {
        Ok(_) => {
            // Persist install_id on successful registration
            state
                .config
                .update(|cfg| {
                    cfg.install_id = Some(install_id);
                })
                .map_err(|e| e.to_string())?;

            tracing::info!(
                miner_key = miner_key,
                wallet = wallet,
                "Device registered with hardwareapi"
            );
            Ok(miner_key)
        }
        Err(e) => {
            // Roll back config on registration failure
            state
                .config
                .update(|cfg| {
                    cfg.miner_key = None;
                    cfg.wallet_address = None;
                })
                .map_err(|e| e.to_string())?;
            Err(format!("API registration failed: {}", e))
        }
    }
}

#[tauri::command]
pub async fn deregister_device(
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    let config = state.config.get();
    if let (Some(ref key), Some(ref id)) = (&config.miner_key, &config.install_id) {
        crate::api::installations::unregister(&state.api_client, key, id)
            .await
            .map_err(|e| format!("API deregistration failed: {}", e))?;
    }

    state
        .config
        .update(|cfg| {
            cfg.miner_key = None;
            cfg.wallet_address = None;
            cfg.install_id = None;
        })
        .map_err(|e| e.to_string())?;

    tracing::info!("Device deregistered");
    Ok(())
}
