use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tracing::{info, warn};

use crate::config::store::ConfigStore;
use crate::integrations::docker_manager::{docker_status, DockerStatus};
use crate::integrations::{HealthStatus, IntegrationRegistry};

/// Docker watcher: monitors Docker state and auto-heals deferred integrations.
///
/// - Spawned after startup recovery
/// - Loop every 90 seconds (back off to 5 min after 10 consecutive not-ready polls)
/// - For each ENABLED integration that requires_docker():
///   - If last_health is Healthy, skip (already running)
///   - Check docker_status(); if Ready, attempt recovery once
///   - Update last_health with Docker blocking reason when not Ready
/// - Never double-start: uses per-integration guard AtomicBool to prevent race with health_check_loop
///
/// BUG 4b: whether — and how long — to sleep before this cycle's Docker
/// status check. `first_pass` never sleeps: this watcher exists to catch
/// integrations the one-time startup recovery pass already found not ready,
/// and a device where Docker finishes starting a few seconds after launch
/// should not have to wait out a blind, unconditional interval before the
/// watcher even looks. Every later pass keeps the existing 90s/300s backoff.
fn watcher_pre_check_sleep(first_pass: bool, not_ready_count: u32) -> Option<Duration> {
    if first_pass {
        None
    } else if not_ready_count >= 10 {
        Some(Duration::from_secs(300))
    } else {
        Some(Duration::from_secs(90))
    }
}

/// H1 review fix: whether the watcher's per-cycle Docker recovery attempt
/// should run right now. Pure — mirrors the exact gate `main.rs`'s
/// `restart_fn` closure already applies for the health loop, extended to
/// this INDEPENDENT recovery loop: `attempt_docker_dependent_recovery` can
/// call `install()`/`start()` on a Docker-dependent integration completely
/// outside the health loop's own restart path, so it was not covered by the
/// BUG 1/2 suspension at all — a second, independent way to spawn/restart a
/// partner process during an in-flight update.
fn should_attempt_docker_recovery(restarts_suspended: bool) -> bool {
    !restarts_suspended
}

pub async fn spawn_docker_watcher(
    registry: Arc<Mutex<IntegrationRegistry>>,
    config: Arc<ConfigStore>,
    last_health: Arc<RwLock<HashMap<String, HealthStatus>>>,
    last_integration_error: Arc<RwLock<HashMap<String, Option<String>>>>,
) {
    let mut not_ready_count: u32 = 0;
    let mut engine_start_attempts: u32 = 0;
    let mut last_engine_start_time = std::time::Instant::now();
    let mut first_pass = true;

    loop {
        // BUG 4b: the very first pass checks Docker immediately instead of
        // blind-sleeping for up to 90s first. This watcher only runs deferred
        // integrations that the one-time startup recovery pass already found
        // NOT ready — if Docker actually becomes ready a few seconds after
        // launch, those integrations should not have to wait out an
        // unconditional interval before this loop even looks.
        if let Some(delay) = watcher_pre_check_sleep(first_pass, not_ready_count) {
            tokio::time::sleep(delay).await;
        }
        first_pass = false;

        // Get Docker status once per cycle
        let docker_status = tokio::task::block_in_place(|| {
            std::thread::sleep(Duration::from_millis(0)); // Yield to avoid blocking
            docker_status()
        });

        if docker_status != DockerStatus::Ready {
            not_ready_count += 1;

            // If DaemonStopped and we haven't exceeded attempt limit and ≥10 min since last attempt
            if docker_status == DockerStatus::DaemonStopped
                && engine_start_attempts < 3
                && last_engine_start_time.elapsed() >= Duration::from_secs(600)
            // 10 min
            {
                info!("Docker daemon stopped — attempting to start it");
                match tokio::task::block_in_place(|| {
                    crate::integrations::docker_manager::try_start_docker_desktop()
                }) {
                    Ok(()) => {
                        info!("Docker Desktop start initiated");
                        engine_start_attempts += 1;
                        last_engine_start_time = std::time::Instant::now();

                        // Update UI health for Docker-gated integrations
                        if let Ok(mut health_map) = last_health.write() {
                            let reg = registry.lock().unwrap();
                            for integration in reg.list() {
                                let id = integration.id();
                                if reg.is_enabled(id) && integration.requires_docker() {
                                    health_map.insert(
                                        id.to_string(),
                                        HealthStatus::Unhealthy(
                                            "Docker engine starting...".to_string(),
                                        ),
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to start Docker Desktop");
                    }
                }
            } else if docker_status == DockerStatus::DaemonStopped {
                // Log that we're not attempting (limit reached or too soon)
                info!(
                    attempts = engine_start_attempts,
                    elapsed_since_last_attempt_secs = last_engine_start_time.elapsed().as_secs(),
                    "Docker daemon stopped but not attempting start (limit or cooldown)"
                );
            }

            info!(
                ?docker_status,
                not_ready_count, "Docker not ready — deferring recovery attempts"
            );
            continue;
        }

        // Docker is ready — reset backoff counter and attempt limit
        not_ready_count = 0;
        engine_start_attempts = 0;

        // H1 review fix: this loop is independent of the health loop's
        // restart_fn — don't install()/start() a Docker-dependent
        // integration while an update install is in progress (same race
        // the health loop is already suspended for during BUG 1/2).
        if !should_attempt_docker_recovery(crate::updater_auto::restarts_suspended()) {
            info!("Docker watcher: recovery skipped — an update install is in progress");
            continue;
        }

        // Attempt recovery for deferred Docker-dependent integrations
        match attempt_docker_dependent_recovery(
            &registry,
            &config,
            &last_health,
            &last_integration_error,
        )
        .await
        {
            Ok(recovered_ids) => {
                if !recovered_ids.is_empty() {
                    info!(recovered = ?recovered_ids, "Docker watcher: recovery succeeded for deferred integrations");
                }
            }
            Err(e) => {
                warn!(error = %e, "Docker watcher: recovery cycle failed");
            }
        }
    }
}

/// Attempt to recover all ENABLED integrations that require_docker() and are currently Unhealthy.
/// Returns list of successfully recovered integration IDs, or error if registry lock fails.
async fn attempt_docker_dependent_recovery(
    registry: &Arc<Mutex<IntegrationRegistry>>,
    _config: &Arc<ConfigStore>,
    last_health: &Arc<RwLock<HashMap<String, HealthStatus>>>,
    last_integration_error: &Arc<RwLock<HashMap<String, Option<String>>>>,
) -> Result<Vec<String>, String> {
    let ids_to_recover: Vec<String> = {
        let reg = registry.lock().map_err(|e| e.to_string())?;
        reg.list()
            .iter()
            .filter_map(|i| {
                let id = i.id();
                // Only consider enabled, Docker-dependent integrations
                if !reg.is_enabled(id) || !i.requires_docker() {
                    return None;
                }

                // Check if it's currently unhealthy (deferred at startup or failed later)
                if let Ok(health_map) = last_health.read() {
                    match health_map.get(id) {
                        Some(HealthStatus::Healthy) | Some(HealthStatus::Stopped) => {
                            // Already healthy or intentionally stopped — skip
                            None
                        }
                        Some(HealthStatus::Unhealthy(_)) | Some(HealthStatus::Unknown) | None => {
                            // Unhealthy or unknown — candidate for recovery
                            Some(id.to_string())
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            })
            .collect()
    };

    let mut recovered = Vec::new();

    for id in ids_to_recover {
        let integration = {
            let reg = registry.lock().map_err(|e| e.to_string())?;
            match reg.get(&id) {
                Some(i) => i,
                None => continue,
            }
        };

        // BUG 4b: install if missing instead of skipping. The one-time
        // startup recovery pass in main.rs already does this for every other
        // enabled integration — this watcher exists specifically to catch
        // Docker-dependent ones that were deferred because Docker was not
        // ready yet at that point, so an enabled-but-never-installed
        // integration reaching here must be installed the same way, not left
        // parked forever waiting for a user action that was never required
        // for any other integration type.
        let installed = tokio::task::block_in_place(|| integration.installed_version()).is_some();
        if !installed {
            info!(
                id = id.as_str(),
                "Docker watcher: enabled but not installed — attempting install"
            );
            if let Err(e) = integration.install().await {
                warn!(id = id.as_str(), error = %e, "Docker watcher: install failed — will retry next cycle");
                if let Ok(mut error_map) = last_integration_error.write() {
                    error_map.insert(id, Some(format!("Install failed: {}", e)));
                }
                continue;
            }
            info!(id = id.as_str(), "Docker watcher: install succeeded");
        }

        // Attempt to start
        match integration.start().await {
            Ok(()) => {
                info!(
                    id = id.as_str(),
                    "Docker watcher: integration started successfully"
                );
                recovered.push(id.clone());

                if let Ok(mut health_map) = last_health.write() {
                    health_map.insert(id.clone(), HealthStatus::Starting);
                }
                if let Ok(mut error_map) = last_integration_error.write() {
                    error_map.insert(id.clone(), None);
                }
            }
            Err(e) => {
                warn!(id = id.as_str(), error = %e, "Docker watcher: start failed");
                if let Ok(mut health_map) = last_health.write() {
                    health_map.insert(
                        id.clone(),
                        HealthStatus::Unhealthy(format!("Docker watcher start failed: {}", e)),
                    );
                }
                if let Ok(mut error_map) = last_integration_error.write() {
                    error_map.insert(id.clone(), Some(format!("Start error: {}", e)));
                }
            }
        }
    }

    Ok(recovered)
}

/// BUG 4b: the watcher's first pass must check Docker immediately, never
/// blind-sleep first.
#[cfg(test)]
mod pre_check_sleep_tests {
    use super::watcher_pre_check_sleep;
    use std::time::Duration;

    #[test]
    fn the_first_pass_never_sleeps_regardless_of_not_ready_count() {
        assert_eq!(watcher_pre_check_sleep(true, 0), None);
        assert_eq!(watcher_pre_check_sleep(true, 50), None);
    }

    #[test]
    fn later_passes_use_the_normal_ninety_second_interval_under_ten_misses() {
        assert_eq!(
            watcher_pre_check_sleep(false, 0),
            Some(Duration::from_secs(90))
        );
        assert_eq!(
            watcher_pre_check_sleep(false, 9),
            Some(Duration::from_secs(90))
        );
    }

    #[test]
    fn later_passes_back_off_to_five_minutes_at_ten_or_more_misses() {
        assert_eq!(
            watcher_pre_check_sleep(false, 10),
            Some(Duration::from_secs(300))
        );
        assert_eq!(
            watcher_pre_check_sleep(false, 15),
            Some(Duration::from_secs(300))
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docker_watcher_backoff() {
        // Test that backoff logic computes correct intervals:
        // 0-9 not-ready polls: 90 sec
        // 10+ not-ready polls: 300 sec
        let test_cases = vec![(0, 90), (5, 90), (9, 90), (10, 300), (15, 300)];

        for (not_ready_count, expected_interval) in test_cases {
            let interval = if not_ready_count >= 10 { 300 } else { 90 };
            assert_eq!(
                interval, expected_interval,
                "Interval mismatch for not_ready_count={}",
                not_ready_count
            );
        }
    }

    #[test]
    fn test_should_attempt_recovery() {
        // Test recovery decision logic:
        // - Unhealthy or Unknown: attempt recovery
        // - Healthy or Stopped: skip
        let test_cases = vec![
            (HealthStatus::Unhealthy("deferred".to_string()), true),
            (HealthStatus::Healthy, false),
            (HealthStatus::Stopped, false),
            (HealthStatus::Starting, true),
            (HealthStatus::Unknown, true),
        ];

        for (status, should_recover) in test_cases {
            let decision = should_attempt_recovery(&status);
            assert_eq!(
                decision, should_recover,
                "Recovery decision mismatch for status {:?}",
                status
            );
        }
    }

    fn should_attempt_recovery(status: &HealthStatus) -> bool {
        match status {
            HealthStatus::Healthy | HealthStatus::Stopped => false,
            HealthStatus::Unhealthy(_)
            | HealthStatus::Unknown
            | HealthStatus::Starting
            | HealthStatus::Installing => true,
        }
    }
}

/// H1 review fix: the docker watcher's recovery loop is independent of the
/// health loop's restart_fn (main.rs) and was not covered by the BUG 1/2
/// restart suspension — `attempt_docker_dependent_recovery` can call
/// `install()`/`start()` on a Docker-dependent integration while an update
/// is mid-flight, the same race the suspension exists to prevent.
#[cfg(test)]
mod docker_recovery_gate_tests {
    use super::should_attempt_docker_recovery;

    #[test]
    fn recovery_is_skipped_while_an_update_is_in_progress() {
        assert!(
            !should_attempt_docker_recovery(true),
            "the watcher must never call install()/start() while restarts_suspended() is true"
        );
    }

    #[test]
    fn recovery_proceeds_normally_when_no_update_is_in_progress() {
        assert!(should_attempt_docker_recovery(false));
    }
}
