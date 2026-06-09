use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DeviceInfo {
    pub miner_key: Option<String>,
    pub wallet_address: Option<String>,
    pub registered: bool,
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

    state
        .config
        .update(|cfg| {
            cfg.miner_key = Some(miner_key.clone());
            cfg.wallet_address = Some(wallet.clone());
        })
        .map_err(|e| e.to_string())?;

    tracing::info!(
        miner_key = miner_key,
        wallet = wallet,
        "Device registered"
    );
    Ok(miner_key)
}
