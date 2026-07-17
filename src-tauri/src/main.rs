#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod commands;
mod config;
mod events;
mod integrations;
mod logging;
mod migration;
mod poc;
mod supervisor;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use api::client::ApiClient;
use config::store::ConfigStore;
use integrations::{HealthStatus, IntegrationRegistry};
use poc::cache::PocCache;
use supervisor::Supervisor;

/// Shared application state, managed by Tauri
pub struct AppState {
    pub config: Arc<ConfigStore>,
    pub registry: Arc<Mutex<IntegrationRegistry>>,
    pub supervisor: Arc<Mutex<Supervisor>>,
    pub api_client: Arc<ApiClient>,
    pub cached_base_reward: Arc<AtomicU64>,
    pub last_health: Arc<RwLock<HashMap<String, HealthStatus>>>,
    pub poc_cache: Arc<PocCache>,
    pub cached_reward_config: Arc<RwLock<Option<crate::api::types::RewardConfig>>>,
    pub cached_stake_tiers: Arc<RwLock<Option<HashMap<String, crate::api::types::StakeTier>>>>,
    pub cached_verified_status: Arc<RwLock<Option<crate::api::types::VerifiedStatus>>>,
    pub reporting_status: Arc<RwLock<crate::api::types::ReportingStatus>>,
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            use tauri::Manager;

            // Register the global app handle early so any module can emit UI
            // events (docker-progress, health) from this point on.
            events::set_app_handle(app.handle().clone());

            // Config store
            let config_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            let config_store = ConfigStore::new(config_dir.clone());
            let config_store = Arc::new(config_store);

            // API client (initial bearer token is the configured token; per-device token applied after registration)
            let cfg = config_store.get();
            let api_client = Arc::new(ApiClient::new(
                cfg.api_base_url.clone(),
                cfg.effective_api_token(),
            ));

            // Device token auto-migration (fire-and-forget, fail-safe)
            {
                let mig_config = config_store.clone();
                let mig_client = api_client.clone();
                tauri::async_runtime::spawn(async move {
                    commands::device::attempt_device_token_migration(&mig_config, &mig_client).await;
                });
            }

            // Process supervisor (created before registry — MysteriumIntegration needs Arc<Mutex<Supervisor>>)
            let log_dir = app
                .path()
                .app_log_dir()
                .expect("failed to resolve app log dir");

            // Initialize logging with scrubbing (rotating 10×5MB files in release)
            logging::init_logging(&log_dir)
                .unwrap_or_else(|e| eprintln!("Warning: failed to initialize logging: {}", e));

            let supervisor = Arc::new(Mutex::new(Supervisor::new(log_dir.clone())));

            // Integration registry
            let mut registry = IntegrationRegistry::new();
            registry.register(Arc::new(integrations::mysterium::MysteriumIntegration {
                api_client: api_client.clone(),
                config: config_store.clone(),
                supervisor: supervisor.clone(),
                log_dir: log_dir.clone(),
            }));
            registry.register(Arc::new(integrations::presearch::PresearchIntegration {
                config: config_store.clone(),
            }));
            registry.register(Arc::new(integrations::diiisco::DiiiscoIntegration {
                api_client: api_client.clone(),
                config: config_store.clone(),
            }));
            registry.register(Arc::new(integrations::space_acres::SpaceAcresIntegration));
            registry.register(Arc::new(integrations::aem::AemIntegration));
            registry.register(Arc::new(integrations::fryvpn::FryVpnIntegration {
                config: config_store.clone(),
                supervisor: supervisor.clone(),
            }));

            // Restore enabled states from config
            for (id, enabled) in &cfg.integrations_enabled {
                registry.set_enabled(id, *enabled);
            }

            let last_health = Arc::new(RwLock::new(HashMap::<String, HealthStatus>::new()));

            // Startup re-install + auto-start moved to an async recovery task
            // below — it must never block app boot (a Docker-dependent
            // integration could otherwise trigger a 500MB download + UAC
            // prompt inside setup, freezing the UI on grey placeholders).

            let integration_count = registry.total_count();
            let registry = Arc::new(Mutex::new(registry));

            let cached_base_reward = Arc::new(AtomicU64::new(0));
            let cached_reward_config = Arc::new(RwLock::new(None::<crate::api::types::RewardConfig>));
            let cached_stake_tiers: Arc<RwLock<Option<HashMap<String, crate::api::types::StakeTier>>>> = Arc::new(RwLock::new(None));
            let cached_verified_status: Arc<RwLock<Option<crate::api::types::VerifiedStatus>>> = Arc::new(RwLock::new(None));
            let poc_cache = Arc::new(PocCache::new(&config_dir));

            // --- Health monitoring: auto-restart with exponential backoff ---
            {
                use integrations::HealthStatus;
                use supervisor::health::{health_check_loop, HealthCheckConfig, HealthEvent};
                use tokio::sync::mpsc;

                let (health_tx, mut health_rx) = mpsc::channel::<HealthEvent>(64);

                let integration_ids: Vec<String> = {
                    let reg = registry.lock().unwrap();
                    reg.list().iter().map(|i| i.id().to_string()).collect()
                };

                for id in integration_ids {
                    let check_registry = registry.clone();
                    let restart_registry = registry.clone();
                    let enabled_registry = registry.clone();
                    let id_check = id.clone();
                    let id_restart = id.clone();
                    let id_enabled = id.clone();
                    let tx = health_tx.clone();

                    let last_health_check = last_health.clone();

                    let check_fn = move || {
                        let reg = check_registry.lock().unwrap();
                        let health = if !reg.is_enabled(&id_check) {
                            HealthStatus::Stopped
                        } else {
                            match reg.get(&id_check) {
                                Some(integration) => tokio::task::block_in_place(|| {
                                    tokio::runtime::Handle::current()
                                        .block_on(integration.health_check())
                                }),
                                None => HealthStatus::Unknown,
                            }
                        };
                        if let Ok(mut map) = last_health_check.write() {
                            map.insert(id_check.clone(), health.clone());
                        }
                        health
                    };

                    let restart_fn = move || {
                        let reg = restart_registry.lock().unwrap();
                        match reg.get(&id_restart) {
                            Some(integration) => {
                                let _ = tokio::task::block_in_place(|| {
                                    tokio::runtime::Handle::current()
                                        .block_on(integration.stop())
                                });
                                tokio::task::block_in_place(|| {
                                    tokio::runtime::Handle::current()
                                        .block_on(integration.start())
                                })
                                .is_ok()
                            }
                            None => false,
                        }
                    };

                    let enabled_fn = move || {
                        enabled_registry
                            .lock()
                            .map(|reg| reg.is_enabled(&id_enabled))
                            .unwrap_or(false)
                    };

                    tauri::async_runtime::spawn(health_check_loop(
                        id,
                        HealthCheckConfig::default(),
                        check_fn,
                        restart_fn,
                        enabled_fn,
                        tx,
                    ));
                }

                // Drop the original sender so the channel closes when all loops exit
                drop(health_tx);

                // Forward health events to frontend
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    use tauri::Emitter;
                    while let Some(event) = health_rx.recv().await {
                        if let Err(e) = app_handle.emit("health-event", &event) {
                            tracing::warn!(error = %e, "Failed to emit health event");
                        }
                    }
                });
            }

            // --- Startup recovery (async — never blocks app boot) ---
            // Re-install enabled-but-missing integrations, then start enabled
            // ones. Docker-dependent integrations are deferred (with a visible
            // reason) unless the engine is already Ready: a Docker download or
            // UAC install must only ever happen on an explicit user toggle.
            {
                let startup_registry = registry.clone();
                let startup_health = last_health.clone();
                tauri::async_runtime::spawn(async move {
                    use integrations::docker_manager::{docker_status, status_user_message, DockerStatus};

                    let ids: Vec<String> = {
                        let reg = startup_registry.lock().unwrap();
                        reg.list()
                            .iter()
                            .map(|i| i.id().to_string())
                            .filter(|id| reg.is_enabled(id))
                            .collect()
                    };

                    for id in ids {
                        let integration = {
                            let reg = startup_registry.lock().unwrap();
                            match reg.get(&id) {
                                Some(i) => i,
                                None => continue,
                            }
                        };

                        if integration.requires_docker() {
                            let status = tokio::task::block_in_place(docker_status);
                            if status != DockerStatus::Ready {
                                tracing::info!(id = id.as_str(), ?status, "Startup: Docker not ready — deferring integration");
                                if let Ok(mut map) = startup_health.write() {
                                    map.insert(id.clone(), HealthStatus::Unhealthy(status_user_message(status)));
                                }
                                continue;
                            }
                        }

                        let installed =
                            tokio::task::block_in_place(|| integration.installed_version()).is_some();
                        if !installed {
                            tracing::info!(id = id.as_str(), "Startup: enabled but not installed — attempting install");
                            if let Err(e) = integration.install().await {
                                tracing::warn!(id = id.as_str(), error = %e, "Startup re-install failed — will retry next launch");
                                if let Ok(mut map) = startup_health.write() {
                                    map.insert(id.clone(), HealthStatus::Unhealthy(format!("Install failed: {}", e)));
                                }
                                continue;
                            }
                            tracing::info!(id = id.as_str(), "Startup re-install succeeded");
                        }

                        match integration.start().await {
                            Ok(()) => {
                                tracing::info!(id = id.as_str(), "Startup: integration started");
                                if let Ok(mut map) = startup_health.write() {
                                    map.insert(id.clone(), HealthStatus::Starting);
                                }
                            }
                            Err(e) => {
                                tracing::warn!(id = id.as_str(), error = %e, "Startup start failed");
                                if let Ok(mut map) = startup_health.write() {
                                    map.insert(id.clone(), HealthStatus::Unhealthy(format!("Start failed: {}", e)));
                                }
                            }
                        }
                    }
                    tracing::info!("Startup recovery pass complete");
                });
            }

            let reporting_status =
                Arc::new(RwLock::new(crate::api::types::ReportingStatus::default()));

            // B6: reconcile OS autostart with the persisted setting at startup.
            {
                use tauri_plugin_autostart::ManagerExt;
                let autolaunch = app.autolaunch();
                let want = cfg.start_on_boot;
                let result = if want {
                    autolaunch.enable()
                } else if autolaunch.is_enabled().unwrap_or(false) {
                    autolaunch.disable()
                } else {
                    Ok(())
                };
                match result {
                    Ok(_) => tracing::info!(start_on_boot = want, "Autostart reconciled"),
                    Err(e) => tracing::warn!(error = %e, "Autostart reconcile failed"),
                }
            }

            // B7: purge legacy PyInstaller _MEI* temp dirs from the pre-Tauri
            // FEM era — stale caches confused key/wallet recovery.
            #[cfg(target_os = "windows")]
            tauri::async_runtime::spawn(async move {
                let Ok(tmp) = std::env::var("TEMP") else { return };
                let Ok(entries) = std::fs::read_dir(&tmp) else { return };
                let mut purged = 0u32;
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with("_MEI") && entry.path().is_dir() {
                        match std::fs::remove_dir_all(entry.path()) {
                            Ok(_) => purged += 1,
                            Err(e) => tracing::debug!(dir = name.as_str(), error = %e, "_MEI purge skipped (in use?)"),
                        }
                    }
                }
                if purged > 0 {
                    tracing::info!(purged = purged, "Legacy _MEI* temp dirs removed");
                }
            });

            // PoC reporter + lease renewal timer (every 10 minutes)
            let poc_config = config_store.clone();
            let poc_registry = registry.clone();
            let poc_client = api_client.clone();
            let poc_base_reward = cached_base_reward.clone();
            let poc_reward_config = cached_reward_config.clone();
            let poc_stake_tiers = cached_stake_tiers.clone();
            let poc_verified_status = cached_verified_status.clone();
            let poc_cache_loop = poc_cache.clone();
            let poc_reporting = reporting_status.clone();
            tauri::async_runtime::spawn(async move {
                // Verify runtime supports block_in_place — panics at first poll if
                // current_thread, not 10 min later in the reward path. Same worker
                // context as compute_health_map.
                tokio::task::block_in_place(|| {});

                let mut interval = tokio::time::interval(Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    let cfg = poc_config.get();
                    if let Some(ref key) = cfg.miner_key {
                        // --- PoC submission (wrapped in {"document": ...}) ---
                        let health_map = poc::reporter::compute_health_map(&poc_registry);
                        let doc = {
                            let reg = poc_registry.lock().unwrap();
                            poc::reporter::build_poc_doc(key, &reg, &health_map)
                        };
                        let wrapped = api::types::PocDocumentWrapper { document: doc };
                        // B2: retry once after a short delay, then record the
                        // outcome so the UI shows truthful reporting state.
                        let mut poc_result = poc_client
                            .put_json(&format!("/PoC/{}/hardware", key), &wrapped)
                            .await;
                        if let Err(ref e) = poc_result {
                            tracing::warn!(error = %e, "PoC submission failed — retrying once");
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            poc_result = poc_client
                                .put_json(&format!("/PoC/{}/hardware", key), &wrapped)
                                .await;
                        }
                        {
                            let mut st = poc_reporting.write().unwrap();
                            st.registered = true;
                            st.last_tick_at = Some(chrono::Utc::now().to_rfc3339());
                            match &poc_result {
                                Ok(_) => {
                                    st.last_poc_ok_at = Some(chrono::Utc::now().to_rfc3339());
                                    st.last_poc_error = None;
                                    st.consecutive_poc_failures = 0;
                                }
                                Err(e) => {
                                    st.last_poc_error = Some(e.to_string());
                                    st.consecutive_poc_failures =
                                        st.consecutive_poc_failures.saturating_add(1);
                                    tracing::warn!(error = %e, failures = st.consecutive_poc_failures, "PoC submission failed after retry");
                                }
                            }
                        }
                        if let Some(slot) = wrapped.document.slots.first() {
                            if let Err(e) = poc_cache_loop.append(slot) {
                                tracing::warn!(error = %e, "PoC slot cache append failed");
                            }
                        }

                        // --- Lease renewal (acquire or renew each tick) ---
                        if let Some(ref install_id) = cfg.install_id {
                            let action = api::types::LeaseAction::default();
                            // Try renew first; if denied (no active lease), acquire
                            let lease_outcome: Result<bool, String> =
                                match api::leases::renew(&poc_client, key, install_id, &action).await {
                                    Ok(resp) if resp.granted => {
                                        tracing::debug!(
                                            miner_key = key.as_str(),
                                            ttl = resp.ttl_seconds,
                                            "Lease renewed"
                                        );
                                        Ok(true)
                                    }
                                    _ => {
                                        // Renew failed or denied — try acquire
                                        match api::leases::acquire(&poc_client, key, install_id, &action).await {
                                            Ok(resp) if resp.granted => {
                                                tracing::info!(
                                                    miner_key = key.as_str(),
                                                    "Lease acquired"
                                                );
                                                Ok(true)
                                            }
                                            Ok(resp) => {
                                                tracing::warn!(
                                                    miner_key = key.as_str(),
                                                    error_code = resp.error_code.as_deref().unwrap_or("none"),
                                                    "Lease denied"
                                                );
                                                Err(format!(
                                                    "lease denied ({})",
                                                    resp.error_code.as_deref().unwrap_or("no active lease")
                                                ))
                                            }
                                            Err(e) => {
                                                tracing::warn!(error = %e, "Lease acquire failed");
                                                Err(e.to_string())
                                            }
                                        }
                                    }
                                };
                            {
                                let mut st = poc_reporting.write().unwrap();
                                match lease_outcome {
                                    Ok(_) => {
                                        st.lease_active = true;
                                        st.lease_error = None;
                                    }
                                    Err(msg) => {
                                        st.lease_active = false;
                                        st.lease_error = Some(msg);
                                    }
                                }
                            }
                            // Surface the fresh snapshot to the UI every tick (B2).
                            let snapshot = poc_reporting.read().unwrap().clone();
                            crate::events::emit("reporting-status", snapshot);
                        }

                        // Refresh base_reward AND reward config from /versions/FEM
                        // (PoC.versions is the single source of truth for reward config)
                        // NOTE: ZEUS00 hardwareapi /versions must handle "linux" platform — verify when Linux E2E phase starts
                        match api::versions::check_version(&poc_client, "FEM", std::env::consts::OS).await {
                            Ok(info) => {
                                if let Some(br) = info.base_reward {
                                    poc_base_reward.store(br.to_bits(), Ordering::Relaxed);
                                }
                                // Build RewardConfig from version response
                                if let (Some(amount), Some(asa_id), Some(name)) = (
                                    info.reward_amount,
                                    info.reward_token_asa_id.as_deref(),
                                    info.reward_token_name.as_deref(),
                                ) {
                                    if let Ok(mut cache) = poc_reward_config.write() {
                                        *cache = Some(crate::api::types::RewardConfig {
                                            key: "FEM".to_string(),
                                            reward_amount: amount,
                                            reward_token_asa_id: asa_id.to_string(),
                                            reward_token_name: name.to_string(),
                                            stake_token_asa_id: String::new(),
                                            stake_token_name: String::new(),
                                        });
                                    }
                                }
                                // Cache stake_tiers from the same response —
                                // no second /versions/FEM round-trip per tick.
                                if let Some(tiers) = info.stake_tiers {
                                    if let Ok(mut cache) = poc_stake_tiers.write() {
                                        *cache = Some(tiers);
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::debug!(error = %e, "version fetch failed, using cached/default");
                            }
                        }

                        // Every 5th tick (~5 min): refresh verification + staking status
                        static VERIFIED_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                        let count = VERIFIED_COUNTER.fetch_add(1, Ordering::Relaxed);
                        if count % 5 == 0 {
                            if let Some(ref key) = cfg.miner_key {
                                match api::credentials::get_verified_status(&poc_client, key).await {
                                    Ok(status) => {
                                        if let Ok(mut cache) = poc_verified_status.write() {
                                            *cache = Some(status);
                                        }
                                    }
                                    Err(e) => {
                                        tracing::debug!(error = %e, "verified status fetch failed");
                                    }
                                }
                            }
                        }
                    }
                }
            });

            // Manage shared state
            app.manage(AppState {
                reporting_status,
                config: config_store,
                registry,
                supervisor,
                api_client,
                cached_base_reward,
                last_health,
                poc_cache,
                cached_reward_config,
                cached_stake_tiers,
                cached_verified_status,
            });

            tracing::info!("FEM initialized — {integration_count} integrations registered");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::debug::export_debug_bundle,
            commands::integration::get_integrations,
            commands::integration::install_integration,
            commands::integration::toggle_integration,
            commands::device::get_device_info,
            commands::device::register_device,
            commands::device::deregister_device,
            commands::device::set_device_name,
            commands::device::get_reporting_status,
            commands::rewards::get_reward_summary,
            commands::rewards::get_poc_slots,
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::system::get_system_status,
            commands::migration::check_migration,
            commands::migration::run_migration,
            commands::updates::check_updates,
            commands::updates::install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running FEM")
}
