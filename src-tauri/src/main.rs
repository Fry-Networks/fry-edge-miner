#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use crate::supervisor::platform::BoundedOutput;

mod api;
mod commands;
mod config;
mod docker_watcher;
mod elevation_gate;
mod events;
mod integrations;
mod logging;
mod migration;
mod poc;
mod security_setup;
mod storage_location;
mod supervisor;
mod system_info;
mod updater_auto;

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
    pub last_integration_error: Arc<RwLock<HashMap<String, Option<String>>>>,
    pub poc_cache: Arc<PocCache>,
    pub cached_reward_config: Arc<RwLock<Option<crate::api::types::RewardConfig>>>,
    pub cached_stake_tiers: Arc<RwLock<Option<HashMap<String, crate::api::types::StakeTier>>>>,
    pub cached_verified_status: Arc<RwLock<Option<crate::api::types::VerifiedStatus>>>,
    pub reporting_status: Arc<RwLock<crate::api::types::ReportingStatus>>,
    pub last_token_recovery: Arc<RwLock<Option<std::time::Instant>>>,
}

/// BUG 12: apply one forwarded health-loop event to the shared `last_health`
/// map that `commands::integration::get_integrations` reads on every poll.
/// Factored out of the event-forwarding loop in `setup()` so the invariant —
/// every event the frontend's `health-event` listener receives must ALSO be
/// persisted, not merely broadcast once — is directly testable without
/// spinning up the whole app. Before this fix, the health loop's bounded
/// startup timeout synthesized a concrete `Unhealthy("Did not finish
/// starting…")` reason and sent it ONLY through the event channel; a poll
/// immediately after (the 30s fallback poll, or any page remount) read
/// straight from `last_health` and saw whatever the next raw `check_fn` tick
/// computed instead — typically a bare `Starting` again — so the card
/// showed the real reason for an instant and reverted to "Starting" forever
/// after.
fn record_forwarded_health_event(
    last_health: &RwLock<HashMap<String, HealthStatus>>,
    event: &supervisor::health::HealthEvent,
) {
    if let Ok(mut map) = last_health.write() {
        map.insert(event.integration_id.clone(), event.status.clone());
    }
}

#[cfg(test)]
mod bug12_health_persistence_tests {
    use super::*;
    use supervisor::health::HealthEvent;

    #[test]
    fn a_forwarded_event_is_persisted_into_the_polled_map() {
        let last_health: RwLock<HashMap<String, HealthStatus>> = RwLock::new(HashMap::new());
        let event = HealthEvent {
            integration_id: "titan".to_string(),
            status: HealthStatus::Unhealthy(
                "Did not finish starting within 180s — retrying".to_string(),
            ),
            restart_count: 1,
        };

        record_forwarded_health_event(&last_health, &event);

        let map = last_health.read().unwrap();
        assert_eq!(
            map.get("titan"),
            Some(&HealthStatus::Unhealthy(
                "Did not finish starting within 180s — retrying".to_string()
            ))
        );
    }

    /// The exact regression this fixes: a startup-timeout event followed
    /// immediately by a "next tick" bare `Starting` event must leave the
    /// LATEST status persisted (last-write-wins) — a poll right after the
    /// timeout event fires sees the timeout reason, matching what the
    /// live event listener already showed, not a value that reverted on
    /// its own without a real state change.
    #[test]
    fn a_later_event_for_the_same_integration_overwrites_the_earlier_one() {
        let last_health: RwLock<HashMap<String, HealthStatus>> = RwLock::new(HashMap::new());
        record_forwarded_health_event(
            &last_health,
            &HealthEvent {
                integration_id: "titan".to_string(),
                status: HealthStatus::Unhealthy(
                    "Did not finish starting within 180s — retrying".to_string(),
                ),
                restart_count: 1,
            },
        );
        record_forwarded_health_event(
            &last_health,
            &HealthEvent {
                integration_id: "titan".to_string(),
                status: HealthStatus::Healthy,
                restart_count: 1,
            },
        );
        assert_eq!(
            last_health.read().unwrap().get("titan"),
            Some(&HealthStatus::Healthy)
        );
    }

    #[test]
    fn events_for_different_integrations_do_not_clobber_each_other() {
        let last_health: RwLock<HashMap<String, HealthStatus>> = RwLock::new(HashMap::new());
        record_forwarded_health_event(
            &last_health,
            &HealthEvent {
                integration_id: "titan".to_string(),
                status: HealthStatus::Unhealthy("missing VC++ runtime".to_string()),
                restart_count: 0,
            },
        );
        record_forwarded_health_event(
            &last_health,
            &HealthEvent {
                integration_id: "mysterium".to_string(),
                status: HealthStatus::Healthy,
                restart_count: 0,
            },
        );
        let map = last_health.read().unwrap();
        assert_eq!(
            map.get("titan"),
            Some(&HealthStatus::Unhealthy("missing VC++ runtime".to_string()))
        );
        assert_eq!(map.get("mysterium"), Some(&HealthStatus::Healthy));
    }
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
            let roaming_path =
                dirs::config_dir().map(|d| d.join("FryEdgeMiner").join("fem_config.json"));
            let config_store = ConfigStore::new(config_dir.clone(), roaming_path);
            let config_store = Arc::new(config_store);

            // API client (initial bearer token is the configured token; per-device token applied after registration)
            let cfg = config_store.get();

            // BUG 1/4: resolve the storage root ONCE, before anything reads it.
            // Ordering is load-bearing — every `registry.register` below, the
            // SpaceAcres SSD/disk warm probe, and the startup orphan sweep all
            // call `partners_base_dir()` and must see the final value.
            integrations::download::init_storage_root(cfg.storage_dir.as_deref());
            let api_client = Arc::new(ApiClient::new(
                cfg.api_base_url.clone(),
                cfg.effective_api_token(),
            ));

            let log_dir = app
                .path()
                .app_log_dir()
                .expect("failed to resolve app log dir");

            // Initialize logging with scrubbing (rotating 10×5MB files in release).
            //
            // This MUST come before the startup hooks spawned below. It used to
            // sit after them, so everything those tasks logged at startup was
            // emitted with no subscriber installed and silently dropped — the
            // version-change heartbeat's own INFO line never appeared in any
            // log while the WARN it emits seconds later did, which is why a
            // token-rotation bug in that hook survived several releases.
            logging::init_logging(&log_dir)
                .unwrap_or_else(|e| eprintln!("Warning: failed to initialize logging: {}", e));

            // Restore the user's debug-logging choice. The sink is always
            // installed; this is what decides whether it writes. Must happen
            // after init_logging, which is what creates the sink.
            logging::debug_sink::set_enabled(config_store.get().debug_logging_enabled);

            // BUG 10/RC4: finish a half-done registration FIRST. The two hooks
            // below both match on (miner_key, install_id) and return early
            // without an install_id, so on a half-registered device they can
            // never make progress until this has run.
            {
                let rc_config = config_store.clone();
                let rc_api = api_client.clone();
                tauri::async_runtime::spawn(async move {
                    commands::device::attempt_registration_completion(&rc_config, &rc_api).await;
                });
            }

            // Device token auto-migration (fire-and-forget, fail-safe)
            {
                let mig_config = config_store.clone();
                let mig_client = api_client.clone();
                tauri::async_runtime::spawn(async move {
                    commands::device::attempt_device_token_migration(&mig_config, &mig_client).await;
                });
            }

            // BUG 11/12: if the installed app version changed since the last
            // report, tell the server right away instead of waiting for the
            // next periodic PoC tick (fire-and-forget, fail-safe — a failure
            // here just means the periodic tick catches it later).
            {
                let ver_config = config_store.clone();
                let ver_client = api_client.clone();
                tauri::async_runtime::spawn(async move {
                    commands::device::attempt_version_change_heartbeat(
                        &ver_config,
                        &ver_client,
                        env!("CARGO_PKG_VERSION"),
                    )
                    .await;
                });
            }

            // Process supervisor (created before registry — MysteriumIntegration needs Arc<Mutex<Supervisor>>)
            let supervisor = Arc::new(Mutex::new(Supervisor::new(log_dir.clone())));

            // Integration registry
            let mut registry = IntegrationRegistry::new();
            registry.register(Arc::new(integrations::mysterium::MysteriumIntegration {
                api_client: api_client.clone(),
                config: config_store.clone(),
                supervisor: supervisor.clone(),
                log_dir: log_dir.clone(),
            }));
            registry.register(Arc::new(integrations::storj::StorjIntegration));
            registry.register(Arc::new(integrations::diiisco::DiiiscoIntegration {
                api_client: api_client.clone(),
                config: config_store.clone(),
            }));
            registry.register(Arc::new(integrations::space_acres::SpaceAcresIntegration::default()));
            registry.register(Arc::new(integrations::aem::AemIntegration::default()));
            registry.register(Arc::new(integrations::fryvpn::FryVpnIntegration {
                config: config_store.clone(),
                api_client: api_client.clone(),
                supervisor: supervisor.clone(),
                log_dir: log_dir.clone(),
            }));
            registry.register(Arc::new(integrations::sentinel::SentinelIntegration));
            registry.register(Arc::new(integrations::titan::TitanIntegration {
                supervisor: supervisor.clone(),
                log_dir: log_dir.clone(),
            }));
            // filecoin_checker retired — see integrations/mod.rs for why.
            registry.register(Arc::new(integrations::iagon::IagonIntegration {
                supervisor: supervisor.clone(),
            }));
            registry.register(Arc::new(integrations::pawns::PawnsIntegration {
                api_client: api_client.clone(),
                config: config_store.clone(),
            }));

            // Warm the SpaceAcres SSD probe before anything can call
            // check_requirements() on a hot path. The first probe spawns up to
            // two PowerShell processes, and check_requirements() runs inside
            // available_count() under the registry mutex — a cold probe there
            // would freeze the UI and stall the PoC reporter behind the lock.
            integrations::space_acres::warm_ssd_probe();

            // BUG 4c: sweep leftover untracked copies of purely
            // supervisor-managed binaries (Myst, Titan) before startup
            // recovery below starts a fresh one — see
            // `updater_auto::STARTUP_ORPHAN_IMAGES` for why SpaceAcres and
            // Olostep are excluded (adopt, don't kill).
            updater_auto::kill_startup_orphans();

            // BUG 1/2: if the PREVIOUS launch of this app was in the middle
            // of an update, confirm it actually landed. Must run before
            // anything else assumes a settled state.
            updater_auto::check_update_outcome_on_launch(&config_dir, env!("CARGO_PKG_VERSION"));

            // BUG 1/2 (part d): one-time elevated Defender exclusion +
            // frynode firewall hardening, once per version. Fire-and-forget —
            // a UAC prompt must never block app boot, and a decline/failure
            // just leaves the manual command logged for the operator.
            {
                let hardening_config = config_store.clone();
                let hardening_cfg = cfg.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::task::block_in_place(|| {
                        let current = env!("CARGO_PKG_VERSION");
                        if !security_setup::should_run_hardening(
                            hardening_cfg.hardening_applied_version.as_deref(),
                            current,
                        ) {
                            return;
                        }
                        let Ok(exe_path) = std::env::current_exe() else { return };
                        let Some(install_dir) = exe_path.parent() else { return };
                        let frynode_path = install_dir.join("resources").join("frynode.exe");
                        let exe_names = ["fry-edge-miner.exe", "frynode.exe"];
                        // B3: boot is not a user action, so this is
                        // `Automatic` — the gate suppresses it and FEM raises
                        // no UAC prompt at startup. The path was already
                        // best-effort with a non-fatal decline branch, so
                        // nothing new can break here; FEM simply ships
                        // unhardened until the user asks for it.
                        match security_setup::run_hardening_elevated(
                            install_dir,
                            &exe_names,
                            &frynode_path,
                            current,
                            crate::elevation_gate::ElevationTrigger::Automatic,
                        ) {
                            Ok(()) => {
                                if let Err(e) = hardening_config.update(|c| {
                                    c.hardening_applied_version = Some(current.to_string());
                                }) {
                                    tracing::warn!(error = %e, "Could not persist hardening_applied_version");
                                }
                            }
                            Err(e) => {
                                let manual = security_setup::manual_hardening_command(install_dir, &exe_names);
                                tracing::warn!(
                                    error = %e,
                                    manual_command = %manual,
                                    "Elevated hardening setup declined or failed — run the manual command as Administrator to apply it yourself"
                                );
                                // B3 defect 4: the log line was the ONLY record
                                // of this. Surface it so the user can see that
                                // hardening is waiting on them.
                                crate::events::emit(
                                    "elevation-required",
                                    serde_json::json!({
                                        "purpose": "hardening",
                                        "reason": e.to_string(),
                                        "manualCommand": manual,
                                    }),
                                );
                            }
                        }
                    });
                });
            }

            // Restore enabled states from config. See BUG 4a in
            // `IntegrationRegistry::restore_enabled_states`: a boot-time
            // `check_requirements()` failure no longer force-disables — it
            // only logs, so the health loop keeps retrying automatically.
            registry.restore_enabled_states(&cfg.integrations_enabled);

            // One-time Presearch cleanup (integration removed — project shut down):
            // best-effort remove any orphaned Docker containers/volume an older FEM
            // created, then drop the stale enabled key. The key's presence marks the
            // cleanup as pending; every step swallows errors (Docker may be absent,
            // containers may not exist) and never blocks boot.
            if cfg.integrations_enabled.contains_key("presearch") {
                let cleanup_config = config_store.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::task::block_in_place(|| {
                        let cfg = cleanup_config.get();
                        let mut names = vec![
                            "presearch-node".to_string(),
                            "presearch-node-unknown".to_string(),
                        ];
                        if let Some(key) = cfg.miner_key.as_ref() {
                            let lower = key.to_lowercase();
                            let suffix = lower[lower.len().saturating_sub(8)..].to_string();
                            names.push(format!("presearch-{}", suffix));
                        }
                        for name in names {
                            let _ = supervisor::platform::command("docker")
                                .args(["rm", "-f", &name])
                                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT);
                        }
                        let _ = supervisor::platform::command("docker")
                            .args(["volume", "rm", "presearch-node-storage"])
                            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT);
                    });
                    if let Err(e) = cleanup_config.update(|c| {
                        c.integrations_enabled.remove("presearch");
                    }) {
                        tracing::warn!(error = %e, "Could not drop stale presearch config key");
                    } else {
                        tracing::info!("Presearch cleanup complete — stale config key dropped");
                    }
                });
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
                        // B1: clone the handle out and DROP the registry guard
                        // before blocking. Holding this std::sync::Mutex across
                        // block_in_place + block_on meant one slow probe (aem
                        // shells out to tasklist) blocked get_integrations,
                        // get_reward_summary and every other integration's
                        // health loop — 10 integrations on a 30s tick held it
                        // near-continuously. Same Arc-clone idiom as
                        // commands/integration.rs.
                        let (enabled, integration) = {
                            let reg = check_registry.lock().unwrap();
                            (reg.is_enabled(&id_check), reg.get(&id_check))
                        };
                        let health = if !enabled {
                            HealthStatus::Stopped
                        } else {
                            match integration {
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
                        // BUG 1/2: once an update install is underway,
                        // release_install_tree has already stopped this
                        // process on purpose — restarting it here would race
                        // download_and_install's file replacement. See
                        // `updater_auto::UPDATE_IN_PROGRESS`.
                        if crate::updater_auto::restarts_suspended() {
                            tracing::info!(
                                id = id_restart.as_str(),
                                "Restart skipped — an update install is in progress"
                            );
                            return false;
                        }
                        // B1: guard dropped before the blocking stop/start, for
                        // the same reason as check_fn above.
                        let integration = {
                            let reg = restart_registry.lock().unwrap();
                            reg.get(&id_restart)
                        };
                        match integration {
                            Some(integration) => {
                                // B6: an integration whose requirements this
                                // machine cannot currently meet — an Algorand
                                // wallet that has not been provisioned yet, a
                                // disk that is too small — fails start() every
                                // single time. Restarting it burns the budget
                                // and re-raises an error the user has already
                                // been shown as `unavailable_reason`.
                                if let Err(reason) = integration.check_requirements() {
                                    tracing::info!(
                                        id = id_restart.as_str(),
                                        reason = %reason,
                                        "Restart skipped — integration is unavailable on this machine"
                                    );
                                    return false;
                                }
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
                //
                // BUG 12 (generic "STARTING only" card — no failure reason
                // survives a refresh): `health_check_loop`'s bounded-startup
                // timeout (`RecoveryAction::StartupTimedOut`) synthesizes a
                // concrete `Unhealthy("Did not finish starting…")` status and
                // sends it ONLY through this event channel — it was never
                // written into `last_health`, the SAME map
                // `commands::integration::get_integrations` reads on every
                // poll. The frontend's live `health-event` listener showed
                // the reason for a moment, then the very next 30s fallback
                // poll (or any page remount) overwrote it with whatever
                // `check_fn`'s own next tick computed — typically a bare
                // `Starting` again, since the timeout is what RESET the
                // starting-ticks counter. The card was correct for an
                // instant and wrong forever after. Persist every forwarded
                // event into `last_health` too, so a poll always sees the
                // same status the event stream already pushed — closes the
                // gap for every integration generically, not just the ones
                // with their own dead-process reason fix (BUG 6/8/9/10).
                let app_handle = app.handle().clone();
                let forwarded_health = last_health.clone();
                tauri::async_runtime::spawn(async move {
                    use tauri::Emitter;
                    while let Some(event) = health_rx.recv().await {
                        record_forwarded_health_event(&forwarded_health, &event);
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

            // --- Docker watcher (async — monitors Docker state and auto-heals deferred integrations) ---
            {
                let watcher_registry = registry.clone();
                let watcher_config = config_store.clone();
                let watcher_health = last_health.clone();
                let watcher_error = Arc::new(RwLock::new(HashMap::<String, Option<String>>::new()));
                tauri::async_runtime::spawn(docker_watcher::spawn_docker_watcher(
                    watcher_registry,
                    watcher_config,
                    watcher_health,
                    watcher_error,
                ));
            }

            // --- Auto-updater (async — checks for updates every 6 hours) ---
            {
                let updater_config = config_store.clone();
                let updater_app = app.handle().clone();
                // B7: the updater needs the supervisor so it can release the
                // partner binaries (frynode.exe et al) before overwriting the
                // install tree.
                let updater_supervisor = supervisor.clone();
                tauri::async_runtime::spawn(updater_auto::spawn_auto_updater(
                    updater_app,
                    updater_config,
                    updater_supervisor,
                ));
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

            // Pawns consent-log retention: superseded entries older than two
            // years move to consent-log-archive.jsonl. Once per launch, before
            // the UI can toggle anything; best effort, never blocks startup.
            tauri::async_runtime::spawn(async {
                crate::integrations::pawns::PawnsIntegration::rotate_consent_log();
            });

            // 401 token-recovery cooldown, shared with the reporting loop.
            let last_token_recovery: Arc<RwLock<Option<std::time::Instant>>> =
                Arc::new(RwLock::new(None));

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
            let poc_recovery_at = last_token_recovery.clone();
            // Commit 3: registration-retry cooldown, same idiom as the 401 one.
            let last_registration_completion: Arc<RwLock<Option<std::time::Instant>>> =
                Arc::new(RwLock::new(None));
            let poc_registration_at = last_registration_completion.clone();
            tauri::async_runtime::spawn(async move {
                // Verify runtime supports block_in_place — panics at first poll if
                // current_thread, not 10 min later in the reward path. Same worker
                // context as compute_health_map.
                tokio::task::block_in_place(|| {});

                let mut interval = tokio::time::interval(Duration::from_secs(60));
                // Failed slot submissions retained for retry once the API is
                // reachable again (in-memory, bounded — see retry_queue.rs).
                let mut retry_queue = poc::retry_queue::PocRetryQueue::new();
                // Consecutive hardware-PUT 401s — feeds the stranded-token
                // recovery path (device with no local device token whose
                // miner_key has a server-side token bound to another install).
                let mut consecutive_401s: u32 = 0;
                loop {
                    interval.tick().await;
                    let cfg = poc_config.get();

                    // Commit 3: a half-registered device (miner_key present,
                    // install_id missing) reconciles here, not only at startup.
                    // Deliberately OUTSIDE the PoC-submission block below: its
                    // problem has nothing to do with whether the PoC POST
                    // succeeded, and the 401-recovery path only runs on failure.
                    if commands::device::should_attempt_registration_completion(
                        cfg.miner_key.is_some(),
                        cfg.install_id.is_some(),
                        *poc_registration_at.read().unwrap(),
                        std::time::Instant::now(),
                        commands::device::REGISTRATION_RETRY_COOLDOWN,
                    ) {
                        *poc_registration_at.write().unwrap() = Some(std::time::Instant::now());
                        commands::device::attempt_registration_completion(&poc_config, &poc_client)
                            .await;
                    }

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
                        // --- 401 recovery: the server no longer recognises this
                        // device token. Clear it, re-register on the shared token,
                        // and retry this tick's submission once.
                        let poc_err_status = poc_result
                            .as_ref()
                            .err()
                            .and_then(commands::device::api_error_status);
                        if poc_err_status == Some(401) {
                            consecutive_401s = consecutive_401s.saturating_add(1);
                        } else {
                            consecutive_401s = 0;
                        }
                        if commands::device::should_attempt_recovery(
                            poc_err_status,
                            cfg.device_token.is_some(),
                            *poc_recovery_at.read().unwrap(),
                            std::time::Instant::now(),
                            commands::device::TOKEN_RECOVERY_COOLDOWN,
                        ) || commands::device::should_attempt_stranded_recovery(
                            poc_err_status,
                            cfg.device_token.is_some(),
                            consecutive_401s,
                            *poc_recovery_at.read().unwrap(),
                            std::time::Instant::now(),
                            commands::device::TOKEN_RECOVERY_COOLDOWN,
                        ) {
                            *poc_recovery_at.write().unwrap() = Some(std::time::Instant::now());
                            // An attempt consumes the 401-run evidence — the
                            // stranded path must re-earn 3 consecutive 401s
                            // before the next attempt (cooldown also applies).
                            consecutive_401s = 0;
                            if commands::device::attempt_token_recovery(&poc_config, &poc_client)
                                .await
                            {
                                poc_result = poc_client
                                    .put_json(&format!("/PoC/{}/hardware", key), &wrapped)
                                    .await;
                                let mut st = poc_reporting.write().unwrap();
                                match &poc_result {
                                    Ok(_) => {
                                        st.last_poc_ok_at = Some(chrono::Utc::now().to_rfc3339());
                                        st.last_poc_error = None;
                                        st.consecutive_poc_failures = 0;
                                        tracing::info!("PoC submission recovered after token refresh");
                                    }
                                    Err(e) => {
                                        st.last_poc_error = Some(e.to_string());
                                        tracing::warn!(error = %e, "PoC submission still failing after token recovery");
                                    }
                                }
                            }
                        }

                        if let Some(slot) = wrapped.document.slots.first() {
                            if let Err(e) = poc_cache_loop.append(slot) {
                                tracing::warn!(error = %e, "PoC slot cache append failed");
                            }
                        }

                        // --- Retry queue: keep failed slots, backfill on recovery ---
                        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
                        match &poc_result {
                            Err(_) => {
                                let slot_number = wrapped
                                    .document
                                    .slots
                                    .first()
                                    .map(|s| s.slot_number)
                                    .unwrap_or_else(poc::reporter::current_slot_number);
                                retry_queue.push(&today, slot_number, wrapped.document.clone());
                                tracing::info!(
                                    queued = retry_queue.len(),
                                    slot = slot_number,
                                    "PoC slot queued for retry"
                                );
                            }
                            Ok(_) => {
                                // Current slot first (heartbeat freshness), then a
                                // bounded backlog drain; stop at the first failure.
                                if !retry_queue.is_empty() {
                                    let batch = retry_queue.take_batch(&today, 12);
                                    let mut resubmitted = 0usize;
                                    for entry in batch {
                                        let rewrapped = api::types::PocDocumentWrapper {
                                            document: entry.doc.clone(),
                                        };
                                        match poc_client
                                            .put_json(&format!("/PoC/{}/hardware", key), &rewrapped)
                                            .await
                                        {
                                            Ok(()) => resubmitted += 1,
                                            Err(e) => {
                                                tracing::warn!(error = %e, slot = entry.slot_number, "Backlog resubmit failed — keeping for next tick");
                                                retry_queue.requeue(entry);
                                                break;
                                            }
                                        }
                                    }
                                    if resubmitted > 0 {
                                        tracing::info!(
                                            resubmitted,
                                            remaining = retry_queue.len(),
                                            "PoC backlog slots resubmitted"
                                        );
                                    }
                                }
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
                        if count.is_multiple_of(5) {
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
                last_integration_error: Arc::new(RwLock::new(HashMap::new())),
                poc_cache,
                cached_reward_config,
                cached_stake_tiers,
                cached_verified_status,
                last_token_recovery,
            });

            tracing::info!("FEM initialized — {integration_count} integrations registered");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::debug::export_debug_bundle,
            commands::debug::get_debug_log_path,
            commands::debug::toggle_debug_logging,
            commands::integration::get_integrations,
            commands::integration::install_integration,
            commands::integration::toggle_integration,
            commands::integration::force_reinstall_integration,
            commands::consent::check_consent,
            commands::consent::grant_consent,
            commands::consent::revoke_consent,
            commands::device::get_device_info,
            commands::device::register_device,
            commands::device::deregister_device,
            commands::device::set_device_name,
            commands::device::get_reporting_status,
            commands::rewards::get_reward_summary,
            commands::rewards::get_poc_slots,
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::settings::set_wallet_address,
            commands::settings::get_storage_location,
            commands::settings::set_storage_location,
            commands::system::get_system_status,
            commands::migration::check_migration,
            commands::migration::run_migration,
            commands::updates::check_updates,
            commands::updates::install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running FEM")
}

/// Commit 3: proof that the registration reconciler is actually WIRED to the
/// PoC tick, not merely defined.
///
/// The pure predicate has had full unit coverage since v0.4.30 (fires after the
/// cooldown, does not fire before it, does not fire when complete or
/// unregistered) — re-running those proves nothing about whether anything calls
/// it. It was dead code for two releases precisely because the predicate was
/// tested and the call site was missing.
///
/// The tick loop cannot be unit-tested (it never returns and owns real I/O), so
/// this asserts on source. Two traps the first version of this test fell into,
/// both now closed:
///   1. it matched its OWN assertion literals, because this file is what it
///      scans — needles are therefore assembled at runtime from fragments;
///   2. it matched the STARTUP call, which already existed and is not what this
///      commit adds — the search is therefore scoped to the PoC tick region.
#[cfg(test)]
mod c3_reconciler_wiring_tests {
    fn code_only(src: &str) -> String {
        src.lines()
            .map(|l| l.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Everything from the PoC loop's own state binding onwards, with this test
    /// module itself excluded. The startup reconciler call sits far above this
    /// marker, so it cannot satisfy the assertions below.
    fn poc_tick_region() -> String {
        let code = code_only(include_str!("main.rs"));
        let start = code
            .find("let poc_recovery_at")
            .expect("the PoC loop binds poc_recovery_at");
        let end = code
            .find("mod c3_reconciler_wiring_tests")
            .unwrap_or(code.len());
        assert!(end > start, "test module must sit after the PoC loop");
        code[start..end].to_string()
    }

    #[test]
    fn the_poc_tick_drives_the_registration_reconciler() {
        let region = poc_tick_region();
        let gate = format!("should_attempt_registration{}completion", '_');
        let call = format!("attempt_registration{}completion(", '_');
        assert!(
            region.contains(&gate),
            "the PoC tick must consult the retry gate, or a half-registered device \
             only ever reconciles at startup"
        );
        assert!(
            region.contains(&call),
            "the PoC tick must actually call the reconciler"
        );
    }

    /// The gate must be consulted BEFORE the reconciler runs, or the cooldown is
    /// decorative and every tick POSTs.
    #[test]
    fn the_retry_is_rate_limited_by_the_gate() {
        let region = poc_tick_region();
        let gate = format!("should_attempt_registration{}completion", '_');
        let call = format!("attempt_registration{}completion(", '_');
        let gate_at = region
            .find(&gate)
            .expect("gate must be referenced in the tick");
        let call_at = region
            .rfind(&call)
            .expect("reconciler must be called in the tick");
        assert!(
            gate_at < call_at,
            "the cooldown gate must be evaluated before the reconciler is invoked"
        );
    }
}

/// B1: the CRT linkage of the shipped Windows binary, and the gate that proves
/// it, are both load-bearing and both invisible from the Rust source — so they
/// are asserted here rather than left to a reviewer to notice.
///
/// v0.4.28 and v0.4.29 imported no CRT DLL. From v0.4.30 the release binary
/// imported VCRUNTIME140.dll and VCRUNTIME140_1.dll, and every user without the
/// VC++ redistributable got "VCRUNTIME140_1.dll was not found" and could not
/// start the app. The linkage was never stated anywhere, so it drifted in
/// silence across a release boundary with the same rustc, the same Cargo.lock
/// and the same [profile.release]. Deleting either the pin or the gate would
/// re-open exactly that door.
#[cfg(test)]
mod b1_crt_linkage_tests {
    const CARGO_CONFIG: &str = include_str!("../.cargo/config.toml");
    const BUILD_WORKFLOW: &str = include_str!("../../.github/workflows/build.yml");

    /// Strip line comments so the prose explaining the pin can never be what
    /// satisfies the assertion — the same guard the other source-scan tests use.
    fn code_only(src: &str) -> String {
        src.lines()
            .map(|l| l.split('#').next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_windows_target_pins_a_static_crt() {
        let code = code_only(CARGO_CONFIG);
        let target = code
            .find("[target.x86_64-pc-windows-msvc]")
            .expect("src-tauri/.cargo/config.toml must carry a table for the shipped target");
        let pin = code
            .find("target-feature=+crt-static")
            .expect("the shipped Windows target must link the CRT statically");
        assert!(
            target < pin,
            "the +crt-static flag must sit under the x86_64-pc-windows-msvc table, \
             not under some other target"
        );
    }

    #[test]
    fn the_release_workflow_runs_the_crt_import_gate_before_it_uploads_anything() {
        let gate = BUILD_WORKFLOW
            .find("check_crt_imports.py")
            .expect("build.yml must run the CRT-import gate");
        let first_upload = BUILD_WORKFLOW
            .find("upload-artifact")
            .expect("build.yml must still upload the installer");
        assert!(
            gate < first_upload,
            "the CRT-import gate must run BEFORE the first upload, or a binary that \
             reproduces the bug is published anyway"
        );
    }

    /// The gate is only worth anything if it covers the binary that actually
    /// failed on users' machines, not just the installer wrapper around it.
    #[test]
    fn the_gate_covers_the_app_binary_the_bundle_and_the_bundled_resources() {
        let step = {
            let at = BUILD_WORKFLOW
                .find("check_crt_imports.py")
                .expect("build.yml must run the CRT-import gate");
            &BUILD_WORKFLOW[at..]
        };
        for target in [
            "target/release/fry-edge-miner.exe",
            "target/release/bundle",
            "src-tauri/resources",
        ] {
            assert!(
                step.contains(target),
                "the CRT-import gate must scan {target}"
            );
        }
    }

    /// The root cause itself: from v0.4.30 the test pass shared the release
    /// target directory, and the release link stopped being decided by the
    /// release build alone.
    #[test]
    fn the_test_pass_does_not_share_the_release_target_directory() {
        let at = BUILD_WORKFLOW
            .find("cargo test --release")
            .expect("build.yml must still run the Rust tests");
        let before = &BUILD_WORKFLOW[..at];
        let export = before
            .rfind("CARGO_TARGET_DIR")
            .expect("the gates must run in their own target directory");
        let cd = before
            .rfind("cd src-tauri")
            .expect("the gates still run from src-tauri");
        assert!(
            cd < export,
            "CARGO_TARGET_DIR must be set after the step enters src-tauri, so the \
             path it names is the one cargo actually uses"
        );
    }
}
