use std::time::Duration;

use rand::rngs::OsRng;
use rand::RngCore;
use serde::Serialize;

use crate::api::types::InstallationHeartbeat;

/// Walk an error's source chain into a single diagnostic string.
fn format_error_chain(err: &dyn std::error::Error) -> String {
    let mut msg = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        msg.push_str(" → ");
        msg.push_str(&cause.to_string());
        source = cause.source();
    }
    msg
}

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

    // Validate config-loaded key format before using
    let miner_key = config
        .miner_key
        .as_ref()
        .and_then(|k| crate::config::miner_key::normalize_fem_key(k).ok());
    if config.miner_key.is_some() && miner_key.is_none() {
        tracing::warn!("Stored miner_key has invalid format; treating as unregistered");
    }

    // Debug diagnostics — no secrets logged
    tracing::debug!(
        key_format_valid = miner_key.is_some(),
        device_token_present = config.device_token.is_some() && !config.device_token.as_ref().unwrap().is_empty(),
        auth_source = if config.device_token.as_ref().filter(|s| !s.is_empty()).is_some() { "device_token" } else { "fallback" },
        "get_device_info diagnostics"
    );

    let mut wallet = config.wallet_address.clone();

    // Auto-populate wallet from hardwareapi if missing locally
    if wallet.is_none() {
        if let Some(ref miner_key) = miner_key {
            match crate::api::credentials::lookup(&state.api_client, miner_key).await {
                Ok(creds) => {
                    if let Some(ref addr) = creds.algo_address {
                        let addr_clone = addr.clone();
                        state
                            .config
                            .update(|cfg| {
                                cfg.wallet_address = Some(addr_clone);
                            })
                            .ok();
                        wallet = Some(addr.clone());
                        tracing::info!("Wallet auto-populated from hardwareapi");
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch credentials for wallet lookup: {}", e);
                }
            }
        }
    }

    Ok(DeviceInfo {
        registered: miner_key.is_some(),
        miner_key,
        wallet_address: wallet,
    })
}

#[tauri::command]
pub async fn register_device(
    wallet: String,
    miner_key: Option<String>,
    state: tauri::State<'_, crate::AppState>,
) -> Result<String, String> {
    crate::config::wallet::validate_address(&wallet).map_err(|e| e.to_string())?;

    let miner_key = match miner_key {
        Some(k) => match crate::config::miner_key::normalize_fem_key(&k) {
            Ok(normalized) => normalized,
            Err(e) => return Err(e),
        },
        None => crate::config::miner_key::generate(),
    };
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

    // Exponential backoff retry on connection-level errors (DNS/TLS/timeout); not on HTTP status errors.
    let mut reg_result = crate::api::installations::register(&state.api_client, &heartbeat).await;
    for attempt in 1..=3u32 {
        match &reg_result {
            Err(crate::api::client::ApiError::Request(_)) => {
                let delay = 2u64.pow(attempt); // 2s, 4s, 8s
                tracing::warn!(
                    attempt = attempt,
                    max_retries = 3,
                    delay_secs = delay,
                    "Registration request failed — retrying"
                );
                tokio::time::sleep(Duration::from_secs(delay)).await;
                reg_result = crate::api::installations::register(&state.api_client, &heartbeat).await;
            }
            _ => break, // success or non-Request error — stop retrying
        }
    }

    match reg_result {
        Ok(response) => {
            // Persist install_id and any per-device token returned by the server
            if let Some(token) = response.device_token {
                state
                    .config
                    .update(|cfg| {
                        cfg.install_id = Some(install_id.clone());
                        cfg.device_token = Some(token.clone());
                    })
                    .map_err(|e| e.to_string())?;
                state.api_client.set_bearer_token(token);
            } else {
                state
                    .config
                    .update(|cfg| {
                        cfg.install_id = Some(install_id);
                    })
                    .map_err(|e| e.to_string())?;
            }

            tracing::info!(
                miner_key = miner_key,
                wallet = wallet,
                "Device registered with hardwareapi"
            );

            // Auto-install all integrations on first registration (non-blocking).
            // Spawn as background task so the frontend sees success immediately.
            if !state.config.get().initial_setup_done {
                let bg_config = std::sync::Arc::clone(&state.config);
                let bg_registry = state.registry.clone();
                tauri::async_runtime::spawn(async move {
                    tracing::info!("First registration — auto-installing integrations in background");

                    let ids: Vec<String> = match bg_registry.lock() {
                        Ok(reg) => reg.list().iter().map(|i| i.id().to_string()).collect(),
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to lock registry for auto-install");
                            return;
                        }
                    };

                    for id in &ids {
                        let integration = match bg_registry.lock() {
                            Ok(reg) => reg.get(id),
                            Err(_) => continue,
                        };

                        let Some(integration) = integration else { continue };

                        let result = tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current()
                                .block_on(integration.install())
                                .map_err(|e| e.to_string())
                        });
                        match result {
                            Ok(_) => tracing::info!(integration = id.as_str(), "Auto-install succeeded"),
                            Err(e) => tracing::warn!(integration = id.as_str(), error = %e, "Auto-install failed — skipping"),
                        }
                    }

                    let installed_ids: Vec<String> = match bg_registry.lock() {
                        Ok(reg) => reg.list()
                            .iter()
                            .filter(|i| i.installed_version().is_some())
                            .map(|i| i.id().to_string())
                            .collect(),
                        Err(_) => Vec::new(),
                    };

                    if let Ok(mut reg) = bg_registry.lock() {
                        for id in &installed_ids {
                            reg.set_enabled(id, true);
                        }
                    }

                    let ids_for_config = installed_ids.clone();
                    if let Err(e) = bg_config.update(|cfg| {
                        cfg.initial_setup_done = true;
                        for id in &ids_for_config {
                            cfg.integrations_enabled.insert(id.clone(), true);
                        }
                    }) {
                        tracing::error!(error = %e, "Failed to persist auto-install state");
                    }

                    tracing::info!(
                        installed = installed_ids.len(),
                        total = ids.len(),
                        "Background auto-install complete"
                    );
                });
            }

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
            Err(format!("API registration failed: {}", format_error_chain(&e)))
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
            cfg.device_token = None;
        })
        .map_err(|e| e.to_string())?;

    let cfg = state.config.get();
    state.api_client.set_bearer_token(cfg.effective_api_token());

    tracing::info!("Device deregistered");
    Ok(())
}

/// Attempt to obtain per-device token at startup if missing.
/// Fail-safe: any error logs and leaves device on shared token.
pub async fn attempt_device_token_migration(
    config: &std::sync::Arc<crate::config::store::ConfigStore>,
    api_client: &std::sync::Arc<crate::api::client::ApiClient>,
) {
    let cfg = config.get();

    // Only migrate if registered (miner_key + install_id) but no device_token
    let (miner_key, install_id) = match (&cfg.miner_key, &cfg.install_id) {
        (Some(k), Some(id)) if cfg.device_token.is_none() => {
            // Validate key format before using in heartbeat
            match crate::config::miner_key::normalize_fem_key(k) {
                Ok(normalized) => (normalized, id.clone()),
                Err(_) => {
                    tracing::warn!("Migration skipped: stored miner_key has invalid format");
                    return;
                }
            }
        }
        _ => return, // nothing to do
    };

    tracing::info!(miner_key = %miner_key, "Attempting device token migration");

    // Field sourcing: identical to register_device (device.rs:78-89)
    let heartbeat = crate::api::types::InstallationHeartbeat {
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

    match crate::api::installations::register(api_client, &heartbeat).await {
        Ok(resp) => {
            if let Some(token) = resp.device_token {
                match config.update(|c| {
                    c.device_token = Some(token.clone());
                }) {
                    Ok(()) => {
                        api_client.set_bearer_token(token);
                        tracing::info!(miner_key = %miner_key, "Device auto-migrated to per-device token");
                    }
                    Err(e) => {
                        tracing::warn!("Failed to persist device_token: {} — continuing on shared token", e);
                    }
                }
            } else {
                tracing::debug!(miner_key = %miner_key, "Server returned no device_token");
            }
        }
        Err(e) => {
            tracing::warn!(
                miner_key = %miner_key,
                error = %e,
                "Device token migration failed — continuing on shared token"
            );
        }
    }
}
