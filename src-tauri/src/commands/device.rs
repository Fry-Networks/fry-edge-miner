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
    pub device_name: Option<String>,
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
        device_name: config.device_name.clone(),
    })
}

#[tauri::command]
pub async fn register_device(
    wallet: String,
    miner_key: Option<String>,
    device_name: Option<String>,
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

    // Snapshot prior state for rollback on failure
    let prior_config = state.config.get();
    let prior_miner_key = prior_config.miner_key.clone();
    let prior_wallet = prior_config.wallet_address.clone();
    let prior_device_token = prior_config.device_token.clone();
    let prior_install_id = prior_config.install_id.clone();

    // B9: idempotent re-registration — if this exact binding already exists
    // locally (same key, same wallet, install + token present) the device is
    // already registered; reuse it instead of re-running the flow.
    if prior_miner_key.as_deref() == Some(miner_key.as_str())
        && prior_wallet.as_deref() == Some(wallet.as_str())
        && prior_install_id.is_some()
        && prior_device_token.is_some()
    {
        tracing::info!(
            miner_key = miner_key.as_str(),
            "Registration idempotent — existing binding reused"
        );
        return Ok(miner_key);
    }

    // Save miner_key + wallet + device_name to config first (install_id saved after API success)
    state
        .config
        .update(|cfg| {
            cfg.miner_key = Some(miner_key.clone());
            cfg.wallet_address = Some(wallet.clone());
            if device_name.is_some() {
                cfg.device_name = device_name.clone();
            }
        })
        .map_err(|e| e.to_string())?;

    // Register with the hardwareapi
    let cfg = state.config.get();
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
        device_name: cfg.device_name.clone(),
    };

    // Exponential backoff retry on connection-level errors (DNS/TLS/timeout)
    // and transient server errors (HTTP 5xx); not on 4xx or decode errors.
    let mut reg_result = crate::api::installations::register(&state.api_client, &heartbeat).await;
    for attempt in 1..=3u32 {
        let retryable = matches!(
            &reg_result,
            Err(crate::api::client::ApiError::Request(_))
                | Err(crate::api::client::ApiError::HttpStatus(500..=599, _))
        );
        if !retryable {
            break; // success or non-retryable error — stop retrying
        }
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
                        cfg.device_token = None;
                    })
                    .map_err(|e| e.to_string())?;
                state.api_client.set_bearer_token(state.config.get().effective_api_token());
            }

            tracing::info!(
                miner_key = miner_key,
                wallet = wallet,
                "Device registered with hardwareapi"
            );

            // Mark initial setup done (explicit opt-in only, no auto-install seeding)
            if !state.config.get().initial_setup_done {
                state
                    .config
                    .update(|cfg| {
                        cfg.initial_setup_done = true;
                    })
                    .ok();
            }

            Ok(miner_key)
        }
        Err(e) => {
            // Restore prior state on registration failure
            state
                .config
                .update(|cfg| {
                    cfg.miner_key = prior_miner_key.clone();
                    cfg.wallet_address = prior_wallet.clone();
                    cfg.device_token = prior_device_token.clone();
                    cfg.install_id = prior_install_id.clone();
                })
                .map_err(|e| e.to_string())?;
            // Reset API client bearer token to reflect restored state
            state.api_client.set_bearer_token(state.config.get().effective_api_token());

            // Connection-level failures get a classified, actionable message
            // (UB-2: some carriers/DNS providers block *.frynetworks.com).
            let msg = match &e {
                crate::api::client::ApiError::Request(req_err) => {
                    crate::api::error_classify::user_facing_registration_error(req_err)
                }
                crate::api::client::ApiError::HttpStatus(code, body)
                    if *code == 409
                        || body.to_lowercase().contains("already registered")
                        || body.contains("IP_ALREADY_REGISTERED") =>
                {
                    if body.contains("IP_ALREADY_REGISTERED") {
                        format!("IP conflict: another miner is already registered from this network address (HTTP {}).\nIf you run multiple devices behind one IP, contact Fry Networks support.\n\nServer detail: {}", code, body)
                    } else {
                        format!("This device key is already registered (HTTP {}).\nIf this is your device, your existing registration is intact — no further action is needed. Open Settings to confirm your miner key and wallet.\n\nServer detail: {}", code, body)
                    }
                }
                crate::api::client::ApiError::HttpStatus(code @ (401 | 403), _) => {
                    format!("Server rejected the request (HTTP {}). Your saved registration was NOT changed. If this persists, check your network/VPN or dashboard.frynetworks.com status, then retry.", code)
                }
                _ => format!("API registration failed: {}", format_error_chain(&e)),
            };
            Err(msg)
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

#[tauri::command]
pub async fn set_device_name(
    name: String,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    state
        .config
        .update(|cfg| {
            cfg.device_name = Some(name.clone());
        })
        .map_err(|e| e.to_string())?;

    tracing::info!(name = %name, "Device name set");
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
        device_name: cfg.device_name.clone(),
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


#[tauri::command]
pub async fn get_reporting_status(
    state: tauri::State<'_, crate::AppState>,
) -> Result<crate::api::types::ReportingStatus, String> {
    Ok(state.reporting_status.read().unwrap().clone())
}
