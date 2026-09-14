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
    /// Unchanged meaning: `miner_key.is_some()`. Tightening this would route a
    /// half-registered device back into the Wizard (BUG 10/RC4).
    pub registered: bool,
    /// BUG 10/RC4: true only when the server confirmed an installation. The UI
    /// uses this to show "finishing registration" instead of silently
    /// pretending a half-registered device is done.
    pub registration_complete: bool,
}

fn generate_install_id() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Serializes registration attempts.
///
/// B1: `register_device` snapshots the prior config up front and restores that
/// snapshot on ANY failure. With two attempts in flight — the retry window is
/// 2+4+8s wide, and the wizard's button is clickable throughout — a failure
/// landing after a sibling SUCCESS restored the pre-success snapshot and wrote
/// `miner_key = None`, which `get_device_info` reports as unregistered and
/// `App.tsx` turns into a bounce back to the onboarding wizard with the user's
/// key and wallet apparently gone. The idempotency check below cannot cover
/// this: it needs `install_id` AND `device_token` already set, so it does
/// nothing on a first registration.
static REGISTRATION_IN_FLIGHT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Whether a failed attempt may roll the config back to its own snapshot.
///
/// Only when the stored key is still the one this attempt wrote. If something
/// else has since written a different key — a concurrent registration that
/// succeeded, or a deregistration — the snapshot is stale and restoring it
/// would destroy the newer binding. Belt-and-braces behind the in-flight lock
/// above, which is what actually serializes the common case.
/// BUG 10/RC4: finish a registration that was left half-done.
///
/// A device with a miner key but no `install_id` is invisible to every other
/// recovery hook in this file (they all match on the `(miner_key, install_id)`
/// tuple and return early). Without this it stays stuck until the user
/// happens to re-run the wizard — the "did not register until after ANOTHER
/// reboot" report. Reuses the SAME heartbeat shape the other hooks build.
pub async fn attempt_registration_completion(
    config: &std::sync::Arc<crate::config::store::ConfigStore>,
    api_client: &std::sync::Arc<crate::api::client::ApiClient>,
) -> bool {
    let cfg = config.get();
    let Some(miner_key) = cfg.miner_key.clone() else {
        return false;
    };
    if cfg.install_id.is_some() {
        return false;
    }

    let install_id = generate_install_id();
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
        Ok(response) => {
            let token = response.device_token.clone();
            if config
                .update(|c| {
                    c.install_id = Some(install_id.clone());
                    if let Some(ref t) = token {
                        c.device_token = Some(t.clone());
                    }
                })
                .is_err()
            {
                return false;
            }
            if let Some(t) = token {
                api_client.set_bearer_token(t);
            }
            tracing::info!(
                miner_key = miner_key.as_str(),
                "Half-registered device reconciled — registration completed"
            );
            crate::events::emit("device-registration-completed", serde_json::json!({}));
            true
        }
        Err(e) => {
            tracing::warn!(error = %e, "Registration completion attempt failed — will retry");
            false
        }
    }
}

/// BUG 10/RC4: how completely is this device registered?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationState {
    /// No miner key at all.
    Unregistered,
    /// A miner key exists but the server never confirmed an installation, so
    /// `install_id` was never persisted. Every startup recovery hook — and
    /// `deregister_device` — is gated on `install_id`, so this device has no
    /// self-service way out.
    Pending,
    /// Key AND install id present.
    Complete,
}

/// PURE. `registered` is derived as `state != Unregistered`, which is
/// BIT-IDENTICAL to the historic `miner_key.is_some()` rule — deliberately so.
/// Tightening `registered` would route a half-registered device back into the
/// Wizard, re-introducing the "keys and wallet go missing" regression class.
pub fn registration_state(miner_key: Option<&str>, install_id: Option<&str>) -> RegistrationState {
    match (miner_key, install_id) {
        (None, _) => RegistrationState::Unregistered,
        (Some(_), None) => RegistrationState::Pending,
        (Some(_), Some(_)) => RegistrationState::Complete,
    }
}

/// How often a half-registered device retries completing its registration.
/// OUT OF SCOPE — DISCOVERED (v0.4.30 gap, not fixed here):
/// this cooldown and `should_attempt_registration_completion` below are unused
/// because v0.4.30 wired `attempt_registration_completion` at STARTUP ONLY and
/// never to the 60s PoC tick it was designed for. A half-registered device
/// therefore reconciles once per launch instead of every 10 minutes. Silenced
/// deliberately rather than deleted, so the designed mechanism is not lost;
/// wiring it up is a behaviour change that needs its own versioned release.
#[allow(dead_code)]
pub const REGISTRATION_RETRY_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(600);

/// PURE: rate-limited reconciliation gate, mirroring the existing
/// `should_attempt_recovery` / TOKEN_RECOVERY_COOLDOWN idiom so there is one
/// retry policy in this file, not two.
#[allow(dead_code)] // see REGISTRATION_RETRY_COOLDOWN above
pub fn should_attempt_registration_completion(
    has_miner_key: bool,
    has_install_id: bool,
    last_attempt: Option<std::time::Instant>,
    now: std::time::Instant,
    cooldown: std::time::Duration,
) -> bool {
    if !has_miner_key || has_install_id {
        return false;
    }
    match last_attempt {
        None => true,
        Some(at) => now.duration_since(at) >= cooldown,
    }
}

/// PURE: does this registration error mean the SERVER already holds this
/// binding, so wiping the local key would trap the user in the Wizard?
///
/// `IP_ALREADY_REGISTERED` is deliberately excluded: that means a DIFFERENT
/// device owns the IP slot, so this key genuinely did not register and the
/// rollback is correct.
pub fn conflict_means_keep_local_binding(status: Option<u16>, body: &str) -> bool {
    let lower = body.to_lowercase();
    if lower.contains("ip_already_registered") {
        return false;
    }
    status == Some(409) || lower.contains("already registered")
}

pub fn should_restore_snapshot(current_key: Option<&str>, this_attempt_key: &str) -> bool {
    current_key == Some(this_attempt_key)
}

/// Decide what `get_device_info` reports for a stored miner key.
///
/// Returns `(key_to_report, format_is_valid_by_todays_rules)`.
///
/// A stored key is reported even when it fails the current validator: the key
/// formats accepted by FEM have changed across releases, and dropping an
/// unrecognised-but-present key made the app treat a registered device as new,
/// bouncing the user into the onboarding Wizard with their key and wallet
/// apparently gone. The stored string is also exactly what the server keys the
/// device document by, so it is the right value to send and display.
pub fn resolve_stored_miner_key(stored: Option<&str>) -> (Option<String>, bool) {
    match stored {
        None => (None, false),
        Some(raw) => match crate::config::miner_key::validate_fem_key_preserve_case(raw) {
            Ok(valid) => (Some(valid), true),
            Err(_) => (Some(raw.to_string()), false),
        },
    }
}

#[tauri::command]
pub async fn get_device_info(
    state: tauri::State<'_, crate::AppState>,
) -> Result<DeviceInfo, String> {
    let config = state.config.get();

    // Validate config-loaded key format before using. Case is preserved:
    // the server-side device doc is keyed by the exact stored string, and a
    // re-cased display key breaks the dashboard's case-sensitive claim (D1).
    let (miner_key, key_format_valid) = resolve_stored_miner_key(config.miner_key.as_deref());
    if config.miner_key.is_some() && !key_format_valid {
        tracing::warn!(
            "Stored miner_key does not match the current format; still reporting the device as registered"
        );
    }

    // Debug diagnostics — no secrets logged
    tracing::debug!(
        key_format_valid,
        device_token_present =
            config.device_token.is_some() && !config.device_token.as_ref().unwrap().is_empty(),
        auth_source = if config
            .device_token
            .as_ref()
            .filter(|s| !s.is_empty())
            .is_some()
        {
            "device_token"
        } else {
            "fallback"
        },
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
        registered: registration_state(miner_key.as_deref(), config.install_id.as_deref())
            != RegistrationState::Unregistered,
        registration_complete: registration_state(
            miner_key.as_deref(),
            config.install_id.as_deref(),
        ) == RegistrationState::Complete,
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
    // B1: held for the whole attempt, so a second registration cannot snapshot
    // state mid-flight and later restore it over this one's result.
    let _in_flight = REGISTRATION_IN_FLIGHT.lock().await;

    crate::config::wallet::validate_address(&wallet).map_err(|e| e.to_string())?;

    let miner_key = match miner_key {
        // Preserve the case of user-provided keys: re-casing registers a NEW
        // identity server-side and orphans the device's existing records (D1).
        Some(k) => match crate::config::miner_key::validate_fem_key_preserve_case(&k) {
            Ok(validated) => validated,
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
                state
                    .api_client
                    .set_bearer_token(state.config.get().effective_api_token());
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
            // BUG 10/RC4: a 409 / "already registered" means the SERVER already
            // holds this binding. Wiping the local key there sends the user back
            // to the Wizard to retype the same key and get the same 409 — and
            // the advice to "open Settings" is unreachable because the Wizard
            // owns the whole screen. Keep the binding so the reconciler and
            // Settings can take over. An IP conflict is excluded: that is a
            // different device owning the slot, where rollback IS correct.
            let keep_binding =
                conflict_means_keep_local_binding(api_error_status(&e), &e.to_string());
            if keep_binding {
                tracing::warn!(
                    miner_key = miner_key.as_str(),
                    "Server reports this device is already registered — keeping the local binding"
                );
            }
            if !keep_binding
                && should_restore_snapshot(state.config.get().miner_key.as_deref(), &miner_key)
            {
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
                state
                    .api_client
                    .set_bearer_token(state.config.get().effective_api_token());
            } else {
                tracing::warn!(
                    "Registration failed, but a different miner key is now stored — leaving it intact rather than restoring a stale snapshot"
                );
            }

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

/// Whether the local registration should be cleared after the server call.
///
/// Normally a failed server-side unregister aborts the whole thing, so the
/// device does not silently detach from a registration the server still
/// believes in. But that left no way out when the call keeps failing: the
/// key stayed in fem_config.json, survived an uninstall/reinstall (the
/// installer does not remove app data), and the device came back registered
/// to the same key forever. `force` is the user explicitly choosing local
/// cleanup anyway, from a second confirmation in Settings.
pub fn should_clear_local(api_ok: bool, force: bool) -> bool {
    api_ok || force
}

#[tauri::command]
pub async fn deregister_device(
    force: Option<bool>,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    let force = force.unwrap_or(false);
    let config = state.config.get();
    // clippy(unused_assignments): the initial `true` was never read -- the
    // branch below decides the value before anything observes it.
    #[allow(unused_assignments)]
    let mut api_ok = true;
    if let (Some(ref key), Some(ref id)) = (&config.miner_key, &config.install_id) {
        if let Err(e) = crate::api::installations::unregister(&state.api_client, key, id).await {
            api_ok = false;
            let msg = format!("API deregistration failed: {}", e);
            if !should_clear_local(api_ok, force) {
                return Err(msg);
            }
            tracing::warn!(error = %e, "Deregistration rejected by the server — clearing local registration anyway at the user's request");
        }
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
            // Validate key format before using in heartbeat. Case preserved —
            // an uppercased key here would upsert a DUPLICATE server-side
            // device doc for lowercase-keyed installs (D1 twin-doc bug).
            match crate::config::miner_key::validate_fem_key_preserve_case(k) {
                Ok(validated) => (validated, id.clone()),
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
                        tracing::warn!(
                            "Failed to persist device_token: {} — continuing on shared token",
                            e
                        );
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

/// Cooldown between automatic device-token recovery attempts.
pub const TOKEN_RECOVERY_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(300);

/// HTTP status carried by an API error, when it has one.
pub fn api_error_status(err: &crate::api::client::ApiError) -> Option<u16> {
    match err {
        crate::api::client::ApiError::HttpStatus(code, _) => Some(*code),
        _ => None,
    }
}

/// Whether a failed request should trigger device-token recovery.
///
/// Only a 401 raised *while holding a per-device token* means the token is
/// stale: `/credentials/{miner_key}/verified` rejects the shared FEM token by
/// design, so a device without a device_token 401s there normally and must not
/// be dragged through recovery. Repeat attempts are bounded by `cooldown`.
pub fn should_attempt_recovery(
    status: Option<u16>,
    has_device_token: bool,
    last_attempt: Option<std::time::Instant>,
    now: std::time::Instant,
    cooldown: std::time::Duration,
) -> bool {
    if status != Some(401) || !has_device_token {
        return false;
    }
    match last_attempt {
        Some(prev) => now.saturating_duration_since(prev) >= cooldown,
        None => true,
    }
}

/// Consecutive PoC hardware-PUT 401s required before a shared-token device is
/// treated as stranded. A single 401 can be by-design (see
/// should_attempt_recovery); a persistent run of them on the hardware PUT is
/// not.
pub const STRANDED_401_THRESHOLD: u32 = 3;

/// Whether a device holding NO per-device token should recover via
/// re-registration.
///
/// Field case (v0.4.7): the server holds a per-device token binding this
/// miner_key (minted by another/older install), so every shared-token
/// hardware PUT 401s forever — one device logged 48/48 failed PUTs in 24h
/// with no way out. Re-registering mints a fresh device token and unsticks
/// it. Requires a RUN of consecutive 401s so the by-design single-401
/// endpoints never trigger it, plus the same cooldown as normal recovery.
pub fn should_attempt_stranded_recovery(
    status: Option<u16>,
    has_device_token: bool,
    consecutive_401s: u32,
    last_attempt: Option<std::time::Instant>,
    now: std::time::Instant,
    cooldown: std::time::Duration,
) -> bool {
    if status != Some(401) || has_device_token {
        return false;
    }
    if consecutive_401s < STRANDED_401_THRESHOLD {
        return false;
    }
    match last_attempt {
        Some(prev) => now.saturating_duration_since(prev) >= cooldown,
        None => true,
    }
}

/// Drop the stored per-device token. Returns true when one was actually held.
pub fn clear_device_token_in(cfg: &mut crate::config::FemConfig) -> bool {
    if cfg.device_token.is_none() {
        return false;
    }
    cfg.device_token = None;
    true
}

/// Recover from a rejected device token: clear it, re-register on the shared
/// token, and store the freshly issued device token. Returns true on success.
///
/// Fail-safe: any failure leaves the device on the shared token and logs; the
/// caller retries on a later tick once the cooldown has elapsed.
pub async fn attempt_token_recovery(
    config: &std::sync::Arc<crate::config::store::ConfigStore>,
    api_client: &std::sync::Arc<crate::api::client::ApiClient>,
) -> bool {
    let cfg = config.get();

    let (miner_key, install_id) = match (&cfg.miner_key, &cfg.install_id) {
        (Some(k), Some(id)) => {
            // Case preserved — see attempt_device_token_migration (D1 twin-doc bug).
            match crate::config::miner_key::validate_fem_key_preserve_case(k) {
                Ok(validated) => (validated, id.clone()),
                Err(_) => {
                    tracing::warn!("Token recovery skipped: stored miner_key has invalid format");
                    return false;
                }
            }
        }
        _ => return false,
    };

    tracing::warn!(
        miner_key = %miner_key,
        "Server rejected the device token (HTTP 401) — clearing it and re-registering"
    );

    // Drop the dead token first so the re-registration goes out on the shared token.
    if let Err(e) = config.update(|c| {
        clear_device_token_in(c);
    }) {
        tracing::warn!(error = %e, "Failed to persist device_token clear — continuing in memory");
    }
    api_client.set_bearer_token(config.get().effective_api_token());

    // Field sourcing: identical to register_device and attempt_device_token_migration.
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
        Ok(resp) => match resp.device_token {
            Some(token) => match config.update(|c| {
                c.device_token = Some(token.clone());
            }) {
                Ok(()) => {
                    api_client.set_bearer_token(token);
                    tracing::info!(miner_key = %miner_key, "Device token recovered — re-registered");
                    true
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Recovered token could not be persisted");
                    false
                }
            },
            None => {
                tracing::warn!(miner_key = %miner_key, "Re-registration returned no device_token");
                false
            }
        },
        Err(e) => {
            tracing::warn!(
                miner_key = %miner_key,
                error = %e,
                "Token recovery failed — retrying after the cooldown"
            );
            false
        }
    }
}

#[tauri::command]
pub async fn get_reporting_status(
    state: tauri::State<'_, crate::AppState>,
) -> Result<crate::api::types::ReportingStatus, String> {
    Ok(state.reporting_status.read().unwrap().clone())
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use std::time::{Duration, Instant};

    const COOLDOWN: Duration = Duration::from_secs(300);

    fn cfg_with(device_token: Option<&str>) -> crate::config::FemConfig {
        let mut cfg = crate::config::FemConfig::default();
        cfg.api_token = "bootstrap-token".to_string();
        cfg.device_token = device_token.map(|s| s.to_string());
        cfg
    }

    // Field reports (georgeparis, tickler): "keys and wallet go missing after
    // FEM runs for a few minutes" / "settings not holding credentials".
    // fem_config.json was intact; get_device_info reported registered:false
    // because the STORED key failed today's validator, and App.tsx routes the
    // whole UI back to the onboarding Wizard on that flag.
    #[test]
    fn a_stored_key_that_fails_todays_validator_still_reports_registered() {
        // Wrong key-part length — the shape an older release could have stored.
        let legacy = "FEM-SHORTKEY123";
        let (reported, valid) = resolve_stored_miner_key(Some(legacy));
        assert!(!valid, "fixture must actually fail the current validator");
        assert_eq!(
            reported.as_deref(),
            Some(legacy),
            "the stored key must still be reported, verbatim"
        );
        assert!(
            reported.is_some(),
            "registered is derived from this being Some — a stored key must never read as unregistered"
        );
    }

    #[test]
    fn a_valid_stored_key_is_reported_unchanged_and_marked_valid() {
        let good = "FEM-15CCB0C7A857E62200373CCF72EAA7D4";
        let (reported, valid) = resolve_stored_miner_key(Some(good));
        assert!(valid);
        assert_eq!(reported.as_deref(), Some(good), "case must be preserved");
    }

    #[test]
    fn no_stored_key_is_genuinely_unregistered() {
        let (reported, valid) = resolve_stored_miner_key(None);
        assert!(reported.is_none(), "a device with no key is unregistered");
        assert!(!valid);
    }

    #[test]
    fn stale_device_token_401_triggers_recovery() {
        assert!(should_attempt_recovery(
            Some(401),
            true,
            None,
            Instant::now(),
            COOLDOWN
        ));
    }

    #[test]
    fn recovery_skipped_without_a_device_token() {
        // /credentials/{miner_key}/verified rejects the shared FEM token by design,
        // so a 401 while holding no device token is expected — not a stale token.
        assert!(!should_attempt_recovery(
            Some(401),
            false,
            None,
            Instant::now(),
            COOLDOWN
        ));
    }

    #[test]
    fn non_401_failures_do_not_trigger_recovery() {
        let now = Instant::now();
        assert!(!should_attempt_recovery(
            Some(500),
            true,
            None,
            now,
            COOLDOWN
        ));
        assert!(!should_attempt_recovery(
            Some(403),
            true,
            None,
            now,
            COOLDOWN
        ));
        assert!(!should_attempt_recovery(None, true, None, now, COOLDOWN));
    }

    #[test]
    fn cooldown_bounds_repeated_recovery_attempts() {
        let base = Instant::now();
        let now = base + Duration::from_secs(600);
        // last attempt 60s ago — still inside the cooldown
        assert!(!should_attempt_recovery(
            Some(401),
            true,
            Some(base + Duration::from_secs(540)),
            now,
            COOLDOWN
        ));
        // last attempt 600s ago — cooldown elapsed
        assert!(should_attempt_recovery(
            Some(401),
            true,
            Some(base),
            now,
            COOLDOWN
        ));
    }

    #[test]
    fn clearing_the_device_token_falls_back_to_the_shared_token() {
        let mut cfg = cfg_with(Some("dead-device-token"));
        assert_eq!(cfg.effective_api_token(), "dead-device-token");
        assert!(clear_device_token_in(&mut cfg));
        assert!(cfg.device_token.is_none());
        assert_eq!(cfg.effective_api_token(), "bootstrap-token");
    }

    #[test]
    fn clearing_is_idempotent_when_no_device_token_is_held() {
        let mut cfg = cfg_with(None);
        assert!(!clear_device_token_in(&mut cfg));
        assert_eq!(cfg.effective_api_token(), "bootstrap-token");
    }

    // --- stranded recovery (v0.4.8): shared-token device stuck on 401s ---

    #[test]
    fn stranded_device_recovers_after_a_run_of_401s() {
        // Repro of the field case: no device token, hardware PUT 401s every
        // tick — before this path existed the device stayed stuck forever
        // (should_attempt_recovery correctly refuses no-token devices).
        assert!(should_attempt_stranded_recovery(
            Some(401),
            false,
            STRANDED_401_THRESHOLD,
            None,
            Instant::now(),
            COOLDOWN
        ));
    }

    #[test]
    fn a_single_401_without_a_token_stays_excluded() {
        // Guards the by-design case documented on should_attempt_recovery.
        assert!(!should_attempt_stranded_recovery(
            Some(401),
            false,
            1,
            None,
            Instant::now(),
            COOLDOWN
        ));
    }

    #[test]
    fn stranded_path_never_fires_while_holding_a_device_token() {
        assert!(!should_attempt_stranded_recovery(
            Some(401),
            true,
            STRANDED_401_THRESHOLD + 5,
            None,
            Instant::now(),
            COOLDOWN
        ));
    }

    #[test]
    fn stranded_recovery_respects_the_cooldown() {
        let base = Instant::now();
        let now = base + Duration::from_secs(600);
        assert!(!should_attempt_stranded_recovery(
            Some(401),
            false,
            STRANDED_401_THRESHOLD,
            Some(base + Duration::from_secs(540)),
            now,
            COOLDOWN
        ));
        assert!(should_attempt_stranded_recovery(
            Some(401),
            false,
            STRANDED_401_THRESHOLD,
            Some(base),
            now,
            COOLDOWN
        ));
    }

    #[test]
    fn non_401_failures_never_trigger_stranded_recovery() {
        let now = Instant::now();
        assert!(!should_attempt_stranded_recovery(
            Some(500),
            false,
            10,
            None,
            now,
            COOLDOWN
        ));
        assert!(!should_attempt_stranded_recovery(
            None, false, 10, None, now, COOLDOWN
        ));
    }

    #[test]
    fn api_error_status_extracts_the_http_code() {
        let unauthorized =
            crate::api::client::ApiError::HttpStatus(401, "Invalid token".to_string());
        assert_eq!(api_error_status(&unauthorized), Some(401));
    }
}

/// B1: overlapping registrations must not roll each other back.
#[cfg(test)]
mod registration_race_tests {
    use super::*;

    const KEY_A: &str = "FEM-15CCB0C7A857E62200373CCF72EAA7D4";
    const KEY_B: &str = "FEM-2A2A2A2A2A2A2A2A2A2A2A2A2A2A2A2A";

    #[test]
    fn a_failed_attempt_rolls_back_its_own_write() {
        // The ordinary case: nothing else touched the config, so the snapshot
        // is still the right thing to restore.
        assert!(should_restore_snapshot(Some(KEY_A), KEY_A));
    }

    #[test]
    fn a_failed_attempt_never_overwrites_a_sibling_that_succeeded() {
        // The field bug: this attempt failed, but another registration has
        // since stored a different key. Restoring here would write
        // miner_key = None and bounce a registered device to the wizard.
        assert!(!should_restore_snapshot(Some(KEY_B), KEY_A));
    }

    #[test]
    fn a_cleared_key_is_left_cleared() {
        // A deregistration landing during the retry window. The user asked for
        // that; a late failure path must not resurrect anything.
        assert!(!should_restore_snapshot(None, KEY_A));
    }

    #[tokio::test]
    async fn registration_attempts_are_serialized() {
        // Proves the guard actually excludes: the second lock cannot be taken
        // while the first is held, and becomes available once it is dropped.
        let first = REGISTRATION_IN_FLIGHT.lock().await;
        assert!(
            REGISTRATION_IN_FLIGHT.try_lock().is_err(),
            "a second registration must not proceed while one is in flight"
        );
        drop(first);
        assert!(
            REGISTRATION_IN_FLIGHT.try_lock().is_ok(),
            "the guard must release once the attempt finishes"
        );
    }
}

/// JuggaCrypto's report: Deregister appeared to do nothing, and reinstalling
/// FEM brought the device back registered to the same key. The server call
/// failed, so the local clear never ran, and the installer does not remove
/// %APPDATA% state — leaving no way to detach the device at all.
#[cfg(test)]
mod deregistration_tests {
    use super::should_clear_local;

    #[test]
    fn a_successful_server_call_clears_local_state() {
        assert!(should_clear_local(true, false));
    }

    #[test]
    fn a_failed_server_call_leaves_local_state_alone_by_default() {
        // Default behaviour is unchanged: don't silently detach from a
        // registration the server still believes in.
        assert!(!should_clear_local(false, false));
    }

    #[test]
    fn force_clears_local_state_even_when_the_server_call_failed() {
        // The user explicitly chose local cleanup from the second confirm.
        assert!(should_clear_local(false, true));
    }
}

/// BUG 11/12: pure decision — does this launch need to report the installed
/// version out of band (immediate heartbeat + lease action) instead of
/// waiting for the next periodic PoC tick to pick it up?
pub fn should_report_version_change(stored: Option<&str>, current: &str) -> bool {
    stored != Some(current)
}

/// BUG 11/12: on every launch of a device that is already registered
/// (miner_key + install_id present), if the installed binary version
/// differs from the last version this device told the server about, send an
/// immediate heartbeat carrying the new `software_version_installed` and
/// renew/acquire the mining lease right away, instead of waiting for the
/// next periodic PoC tick (which previously left the dashboard showing a
/// stale version, sometimes indefinitely if that tick's submission kept
/// failing). Only persists `last_reported_version` when the heartbeat
/// actually succeeds, so a failed attempt retries on the next launch.
pub async fn attempt_version_change_heartbeat(
    config: &std::sync::Arc<crate::config::store::ConfigStore>,
    api_client: &std::sync::Arc<crate::api::client::ApiClient>,
    current_version: &str,
) {
    let cfg = config.get();
    let (miner_key, install_id) = match (&cfg.miner_key, &cfg.install_id) {
        (Some(k), Some(id)) => (k.clone(), id.clone()),
        _ => return, // not registered yet — register_device reports the version itself
    };

    if !should_report_version_change(cfg.last_reported_version.as_deref(), current_version) {
        return;
    }

    tracing::info!(
        miner_key = %miner_key,
        from = cfg.last_reported_version.as_deref().unwrap_or("(none)"),
        to = current_version,
        "App version changed since last report — sending immediate heartbeat + lease action"
    );

    let heartbeat = crate::api::types::InstallationHeartbeat {
        miner_key: miner_key.clone(),
        install_id: install_id.clone(),
        miner_code: Some("FEM".to_string()),
        software_version_installed: Some(current_version.to_string()),
        poc_version_installed: Some("1.0.0".to_string()),
        hostname: std::env::var("COMPUTERNAME")
            .ok()
            .or_else(|| std::env::var("HOSTNAME").ok()),
        os: Some(std::env::consts::OS.to_string()),
        is_installed: Some(true),
        device_name: cfg.device_name.clone(),
    };

    let heartbeat_ok = match crate::api::installations::register(api_client, &heartbeat).await {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!(error = %e, "Version-change heartbeat failed — will retry on next launch");
            false
        }
    };

    // Best-effort lease action. A failure here doesn't block persisting the
    // heartbeat success — the regular PoC tick's own lease renewal covers it.
    let action = crate::api::types::LeaseAction::default();
    let renew_result =
        crate::api::leases::renew(api_client, &miner_key, &install_id, &action).await;
    let lease_ok = match &renew_result {
        Ok(resp) if resp.granted => true,
        _ => {
            match crate::api::leases::acquire(api_client, &miner_key, &install_id, &action).await {
                Ok(resp) => resp.granted,
                Err(_) => false,
            }
        }
    };
    if !lease_ok {
        tracing::warn!(miner_key = %miner_key, "Version-change lease action did not grant — the next PoC tick will retry");
    }

    if heartbeat_ok {
        if let Err(e) =
            config.update(|c| c.last_reported_version = Some(current_version.to_string()))
        {
            tracing::warn!(error = %e, "Failed to persist last_reported_version");
        }
    }
}

/// BUG 11/12 (Discord: "auto-update disconnects the device from the
/// dashboard, have to touch each device" / rozell 9/03; "config overwritten
/// on restart" / georgeparis 8/20): a device already on a per-device token
/// only reported its installed version via the periodic PoC tick, so the
/// dashboard could show a stale `software_version_installed` for up to a
/// full tick interval after an update — and if that tick's PUT failed for
/// any reason, indefinitely, until someone manually re-touched the device.
/// `should_report_version_change` is the pure decision this launch's startup
/// hook uses to force an immediate heartbeat + lease action instead of
/// waiting for the next tick.
#[cfg(test)]
mod version_change_tests {
    use super::should_report_version_change;

    #[test]
    fn a_version_bump_since_the_last_report_must_report() {
        assert!(should_report_version_change(Some("0.4.27"), "0.4.28"));
    }

    #[test]
    fn the_same_version_as_last_report_must_not_report_again() {
        assert!(!should_report_version_change(Some("0.4.28"), "0.4.28"));
    }

    #[test]
    fn no_prior_report_on_a_registered_device_must_report() {
        // e.g. a device registered before this field existed.
        assert!(should_report_version_change(None, "0.4.28"));
    }
}

/// BUG 10 / RC4 (RailgunDude): "On reinstall + miner-key entry, the device did
/// not register until after ANOTHER reboot."
///
/// Two compounding traps.
///
/// (1) HALF-REGISTRATION. `register_device` writes `miner_key` BEFORE the API
/// call but persists `install_id` only on success. `DeviceInfo.registered` is
/// `miner_key.is_some()`, and `App.tsx` uses exactly that to choose AppShell
/// over Wizard — so after a failed first registration the wizard never comes
/// back, while all three startup recovery hooks AND `deregister_device` are
/// gated on `install_id.is_some()` and return early. The device is stuck with
/// no self-service exit at all.
///
/// (2) THE 409 TRAP. On a reinstall the server returns 409 / "already
/// registered" for the key the user just re-entered. That is non-retryable, so
/// the error arm rolls `miner_key` back to `None`, which puts the user back in
/// the Wizard to retype the same key and get the same 409. The error text says
/// "Open Settings to confirm your miner key" — but Settings is unreachable,
/// because the Wizard owns the whole screen. Independently corroborated: a peer
/// measured 485 `main.devices` rows with `is_registered:true` and NO address,
/// which is exactly the population that answers ALREADY_REGISTERED.
#[cfg(test)]
mod bug10_registration_recovery_tests {
    use super::*;

    #[test]
    fn a_key_without_an_install_id_is_pending_not_complete() {
        assert_eq!(
            registration_state(Some("FEM-A"), None),
            RegistrationState::Pending
        );
        assert_eq!(
            registration_state(Some("FEM-A"), Some("i-1")),
            RegistrationState::Complete
        );
        assert_eq!(
            registration_state(None, None),
            RegistrationState::Unregistered
        );
        // A stray install_id with no key is not a registration.
        assert_eq!(
            registration_state(None, Some("i-1")),
            RegistrationState::Unregistered
        );
    }

    /// SAFETY NET: `registered` must stay bit-identical to the old rule, or a
    /// half-registered device gets routed back into the Wizard — the exact
    /// regression class `resolve_stored_miner_key` and `should_restore_snapshot`
    /// were written to fix.
    #[test]
    fn the_registered_flag_is_bit_identical_to_the_old_rule() {
        for (k, i) in [
            (None, None),
            (None, Some("i")),
            (Some("FEM-A"), None),
            (Some("FEM-A"), Some("i")),
        ] {
            let old_rule = k.is_some();
            assert_eq!(
                registration_state(k, i) != RegistrationState::Unregistered,
                old_rule,
                "registered must not change meaning for ({k:?}, {i:?})"
            );
        }
    }

    #[test]
    fn a_half_registered_device_is_reconciled_after_the_cooldown() {
        let now = std::time::Instant::now();
        let cooldown = std::time::Duration::from_secs(600);
        // Never attempted -> go now.
        assert!(should_attempt_registration_completion(
            true, false, None, now, cooldown
        ));
        // Just attempted -> wait.
        assert!(!should_attempt_registration_completion(
            true,
            false,
            Some(now),
            now,
            cooldown
        ));
        // Cooldown elapsed -> go again.
        assert!(should_attempt_registration_completion(
            true,
            false,
            Some(now - cooldown - std::time::Duration::from_secs(1)),
            now,
            cooldown
        ));
    }

    #[test]
    fn a_complete_or_unregistered_device_is_never_reconciled() {
        let now = std::time::Instant::now();
        let cooldown = std::time::Duration::from_secs(600);
        assert!(!should_attempt_registration_completion(
            true, true, None, now, cooldown
        ));
        assert!(!should_attempt_registration_completion(
            false, false, None, now, cooldown
        ));
    }

    #[test]
    fn an_already_registered_conflict_keeps_the_local_binding() {
        assert!(conflict_means_keep_local_binding(Some(409), ""));
        assert!(conflict_means_keep_local_binding(
            Some(400),
            "device already registered"
        ));
        assert!(conflict_means_keep_local_binding(
            None,
            "Already Registered"
        ));
    }

    /// An IP conflict is a DIFFERENT device owning the slot — this key genuinely
    /// did not register, so rolling back is correct there.
    #[test]
    fn an_ip_conflict_still_rolls_back() {
        assert!(!conflict_means_keep_local_binding(
            Some(409),
            "IP_ALREADY_REGISTERED"
        ));
        assert!(!conflict_means_keep_local_binding(
            Some(500),
            "internal error"
        ));
        assert!(!conflict_means_keep_local_binding(
            Some(401),
            "unauthorized"
        ));
    }
}
