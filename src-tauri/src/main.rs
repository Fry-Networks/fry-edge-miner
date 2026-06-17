#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod commands;
mod config;
mod integrations;
mod migration;
mod poc;
mod supervisor;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use api::client::ApiClient;
use config::store::ConfigStore;
use integrations::IntegrationRegistry;
use supervisor::Supervisor;

/// Shared application state, managed by Tauri
pub struct AppState {
    pub config: Arc<ConfigStore>,
    pub registry: Arc<Mutex<IntegrationRegistry>>,
    pub supervisor: Arc<Mutex<Supervisor>>,
    pub api_client: Arc<ApiClient>,
    pub cached_base_reward: Arc<AtomicU64>,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            use tauri::Manager;

            // Config store
            let config_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            let config_store =
                ConfigStore::new(config_dir).expect("failed to initialize config store");
            let config_store = Arc::new(config_store);

            // API client (bearer token baked at compile time via FEM_API_TOKEN env var)
            let cfg = config_store.get();
            let api_client = Arc::new(ApiClient::new(cfg.api_base_url.clone(), cfg.api_token.clone()));

            // Process supervisor (created before registry — MysteriumIntegration needs Arc<Mutex<Supervisor>>)
            let log_dir = app
                .path()
                .app_log_dir()
                .expect("failed to resolve app log dir");
            let supervisor = Arc::new(Mutex::new(Supervisor::new(log_dir.clone())));

            // Integration registry
            let mut registry = IntegrationRegistry::new();
            registry.register(Box::new(integrations::mysterium::MysteriumIntegration {
                api_client: api_client.clone(),
                config: config_store.clone(),
                supervisor: supervisor.clone(),
                log_dir: log_dir.clone(),
            }));
            registry.register(Box::new(integrations::presearch::PresearchIntegration));
            registry.register(Box::new(integrations::diiisco::DiiiscoIntegration {
                api_client: api_client.clone(),
                config: config_store.clone(),
            }));
            registry.register(Box::new(integrations::space_acres::SpaceAcresIntegration));
            registry.register(Box::new(integrations::aem::AemIntegration));

            // Restore enabled states from config
            for (id, enabled) in &cfg.integrations_enabled {
                registry.set_enabled(id, *enabled);
            }
            let integration_count = registry.total_count();
            let registry = Arc::new(Mutex::new(registry));

            let cached_base_reward = Arc::new(AtomicU64::new(0));

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
                    let id_check = id.clone();
                    let id_restart = id.clone();
                    let tx = health_tx.clone();

                    let check_fn = move || {
                        let reg = check_registry.lock().unwrap();
                        if !reg.is_enabled(&id_check) {
                            return HealthStatus::Stopped;
                        }
                        match reg.get(&id_check) {
                            Some(integration) => tokio::task::block_in_place(|| {
                                tokio::runtime::Handle::current()
                                    .block_on(integration.health_check())
                            }),
                            None => HealthStatus::Unknown,
                        }
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

                    tauri::async_runtime::spawn(health_check_loop(
                        id,
                        HealthCheckConfig::default(),
                        check_fn,
                        restart_fn,
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

            // PoC reporter + lease renewal timer (every 10 minutes)
            let poc_config = config_store.clone();
            let poc_registry = registry.clone();
            let poc_client = api_client.clone();
            let poc_base_reward = cached_base_reward.clone();
            tauri::async_runtime::spawn(async move {
                // Verify runtime supports block_in_place — panics at first poll if
                // current_thread, not 10 min later in the reward path. Same worker
                // context as compute_health_map.
                tokio::task::block_in_place(|| {});

                let mut interval = tokio::time::interval(Duration::from_secs(600));
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
                        if let Err(e) = poc_client
                            .put_json(&format!("/PoC/{}/hardware", key), &wrapped)
                            .await
                        {
                            tracing::warn!(error = %e, "PoC submission failed");
                        }

                        // --- Lease renewal (acquire or renew each tick) ---
                        if let Some(ref install_id) = cfg.install_id {
                            let action = api::types::LeaseAction::default();
                            // Try renew first; if denied (no active lease), acquire
                            match api::leases::renew(&poc_client, key, install_id, &action).await {
                                Ok(resp) if resp.granted => {
                                    tracing::debug!(
                                        miner_key = key.as_str(),
                                        ttl = resp.ttl_seconds,
                                        "Lease renewed"
                                    );
                                }
                                _ => {
                                    // Renew failed or denied — try acquire
                                    match api::leases::acquire(&poc_client, key, install_id, &action).await {
                                        Ok(resp) if resp.granted => {
                                            tracing::info!(
                                                miner_key = key.as_str(),
                                                "Lease acquired"
                                            );
                                        }
                                        Ok(resp) => {
                                            tracing::warn!(
                                                miner_key = key.as_str(),
                                                error_code = resp.error_code.as_deref().unwrap_or("none"),
                                                "Lease denied"
                                            );
                                        }
                                        Err(e) => {
                                            tracing::warn!(error = %e, "Lease acquire failed");
                                        }
                                    }
                                }
                            }
                        }

                        // Refresh cached base_reward from /versions/FEM
                        match api::versions::check_version(&poc_client, "FEM", "windows").await {
                            Ok(info) => {
                                if let Some(br) = info.base_reward {
                                    poc_base_reward.store(br.to_bits(), Ordering::Relaxed);
                                }
                            }
                            Err(e) => {
                                tracing::debug!(error = %e, "base_reward fetch failed, using cached/default");
                            }
                        }
                    }
                }
            });

            // Manage shared state
            app.manage(AppState {
                config: config_store,
                registry,
                supervisor,
                api_client,
                cached_base_reward,
            });

            tracing::info!("FEM initialized — {integration_count} integrations registered");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::integration::get_integrations,
            commands::integration::install_integration,
            commands::integration::toggle_integration,
            commands::device::get_device_info,
            commands::device::register_device,
            commands::device::deregister_device,
            commands::rewards::get_reward_summary,
            commands::rewards::get_poc_slots,
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::migration::check_migration,
            commands::migration::run_migration,
        ])
        .run(tauri::generate_context!())
        .expect("error while running FEM")
}
