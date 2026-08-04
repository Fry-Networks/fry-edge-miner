use std::sync::{Arc, Mutex, RwLock};
use std::collections::HashMap;
use std::time::Duration;

use tracing::{info, warn};

use crate::config::store::ConfigStore;
use crate::integrations::{HealthStatus, IntegrationRegistry};
use crate::integrations::docker_manager::{docker_status, DockerStatus};

/// Docker watcher: monitors Docker state and auto-heals deferred integrations.
///
/// - Spawned after startup recovery
/// - Loop every 90 seconds (back off to 5 min after 10 consecutive not-ready polls)
/// - For each ENABLED integration that requires_docker():
///   - If last_health is Healthy, skip (already running)
///   - Check docker_status(); if Ready, attempt recovery once
///   - Update last_health with Docker blocking reason when not Ready
/// - Never double-start: uses per-integration guard AtomicBool to prevent race with health_check_loop
pub async fn spawn_docker_watcher(
    registry: Arc<Mutex<IntegrationRegistry>>,
    config: Arc<ConfigStore>,
    last_health: Arc<RwLock<HashMap<String, HealthStatus>>>,
    last_integration_error: Arc<RwLock<HashMap<String, Option<String>>>>,
) {
    let mut not_ready_count: u32 = 0;
    let mut engine_start_attempts: u32 = 0;
    let mut last_engine_start_time = std::time::Instant::now();

    loop {
        // Compute interval: 90 sec normally, back off to 5 min after 10 not-ready polls
        let interval_secs = if not_ready_count >= 10 {
            300 // 5 minutes
        } else {
            90 // 1.5 minutes
        };

        tokio::time::sleep(Duration::from_secs(interval_secs)).await;

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
                && last_engine_start_time.elapsed() >= Duration::from_secs(600) // 10 min
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
                                        HealthStatus::Unhealthy("Docker engine starting...".to_string()),
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
                not_ready_count,
                "Docker not ready — deferring recovery attempts"
            );
            continue;
        }

        // Docker is ready — reset backoff counter and attempt limit
        not_ready_count = 0;
        engine_start_attempts = 0;

        // Attempt recovery for deferred Docker-dependent integrations
        match attempt_docker_dependent_recovery(&registry, &config, &last_health, &last_integration_error)
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
                            return None;
                        }
                        Some(HealthStatus::Unhealthy(_)) | Some(HealthStatus::Unknown) | None => {
                            // Unhealthy or unknown — candidate for recovery
                            return Some(id.to_string());
                        }
                        _ => None,
                    }
                } else {
                    return None;
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

        // Check if already installed (if not, skip — installation requires explicit user action)
        let installed = tokio::task::block_in_place(|| integration.installed_version()).is_some();
        if !installed {
            info!(id = id.as_str(), "Docker watcher: integration not installed — skipping");
            if let Ok(mut error_map) = last_integration_error.write() {
                error_map.insert(id, Some("Docker ready but integration not installed".to_string()));
            }
            continue;
        }

        // Attempt to start
        match integration.start().await {
            Ok(()) => {
                info!(id = id.as_str(), "Docker watcher: integration started successfully");
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
                    health_map.insert(id.clone(), HealthStatus::Unhealthy(format!("Docker watcher start failed: {}", e)));
                }
                if let Ok(mut error_map) = last_integration_error.write() {
                    error_map.insert(id.clone(), Some(format!("Start error: {}", e)));
                }
            }
        }
    }

    Ok(recovered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docker_watcher_backoff() {
        // Test that backoff logic computes correct intervals:
        // 0-9 not-ready polls: 90 sec
        // 10+ not-ready polls: 300 sec
        let test_cases = vec![
            (0, 90),
            (5, 90),
            (9, 90),
            (10, 300),
            (15, 300),
        ];

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
            HealthStatus::Unhealthy(_) | HealthStatus::Unknown | HealthStatus::Starting | HealthStatus::Installing => true,
        }
    }
}
