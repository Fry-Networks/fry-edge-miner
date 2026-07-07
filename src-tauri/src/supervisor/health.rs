use std::time::Duration;

use serde::Serialize;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::integrations::HealthStatus;

/// Event emitted by the health check loop
#[derive(Debug, Clone, Serialize)]
pub struct HealthEvent {
    pub integration_id: String,
    pub status: HealthStatus,
    pub restart_count: u32,
}

/// Configuration for health check behavior
pub struct HealthCheckConfig {
    pub check_interval: Duration,
    pub max_restarts: u32,
    pub backoff_base: Duration,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(30),
            max_restarts: 3,
            backoff_base: Duration::from_secs(5),
        }
    }
}

/// Run a health check loop for a single integration.
/// Calls `check_fn` periodically and sends events to `tx`.
///
/// Recovery rules:
/// - `Unhealthy`, or `Stopped` while the integration is enabled, triggers
///   `restart_fn` with exponential backoff, up to `max_restarts` in a row.
/// - After the budget is exhausted the loop KEEPS RUNNING (observe-only) and
///   re-arms one restart attempt every `REARM_TICKS` checks, so a long Docker
///   outage or reboot self-heals instead of leaving stale state forever.
/// - Any `Healthy` result resets the budget.
pub async fn health_check_loop<C, R, E>(
    integration_id: String,
    config: HealthCheckConfig,
    check_fn: C,
    restart_fn: R,
    enabled_fn: E,
    tx: mpsc::Sender<HealthEvent>,
) where
    C: Fn() -> HealthStatus + Send + 'static,
    R: Fn() -> bool + Send + 'static,
    E: Fn() -> bool + Send + 'static,
{
    // Grant one fresh restart attempt every N checks after the budget is
    // spent (N * check_interval ≈ 5 minutes with defaults).
    const REARM_TICKS: u32 = 10;

    let mut restart_count: u32 = 0;
    let mut last_status = HealthStatus::Unknown;
    let mut ticks_since_exhausted: u32 = 0;

    loop {
        tokio::time::sleep(config.check_interval).await;

        let status = check_fn();
        let status_changed = std::mem::discriminant(&status) != std::mem::discriminant(&last_status);

        if status_changed {
            info!(
                integration = integration_id,
                ?status,
                "Health status changed"
            );
            let _ = tx
                .send(HealthEvent {
                    integration_id: integration_id.clone(),
                    status: status.clone(),
                    restart_count,
                })
                .await;
        }

        let needs_restart = match &status {
            HealthStatus::Unhealthy(_) => true,
            HealthStatus::Stopped => enabled_fn(),
            _ => false,
        };

        if matches!(status, HealthStatus::Healthy) {
            restart_count = 0;
            ticks_since_exhausted = 0;
        } else if needs_restart {
            if restart_count < config.max_restarts {
                restart_count += 1;
                let backoff = config.backoff_base * 3u32.pow(restart_count - 1);
                warn!(
                    integration = integration_id,
                    attempt = restart_count,
                    backoff_secs = backoff.as_secs(),
                    "Attempting restart"
                );
                tokio::time::sleep(backoff).await;
                if restart_fn() {
                    info!(integration = integration_id, "Restart succeeded");
                } else {
                    warn!(integration = integration_id, "Restart failed");
                }
            } else {
                ticks_since_exhausted += 1;
                if ticks_since_exhausted == 1 {
                    warn!(
                        integration = integration_id,
                        "Restart budget exhausted — pausing automatic restarts, will retry periodically"
                    );
                    let reason = match &status {
                        HealthStatus::Unhealthy(r) => r.clone(),
                        _ => "process not running".to_string(),
                    };
                    let _ = tx
                        .send(HealthEvent {
                            integration_id: integration_id.clone(),
                            status: HealthStatus::Unhealthy(format!(
                                "{} — automatic restarts paused, retrying periodically",
                                reason
                            )),
                            restart_count,
                        })
                        .await;
                } else if ticks_since_exhausted >= REARM_TICKS {
                    ticks_since_exhausted = 0;
                    restart_count = config.max_restarts - 1; // grant one attempt
                }
            }
        }

        last_status = status;
    }
}
