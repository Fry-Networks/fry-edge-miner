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
/// On unhealthy status, calls `restart_fn` with exponential backoff.
pub async fn health_check_loop<C, R>(
    integration_id: String,
    config: HealthCheckConfig,
    check_fn: C,
    restart_fn: R,
    tx: mpsc::Sender<HealthEvent>,
) where
    C: Fn() -> HealthStatus + Send + 'static,
    R: Fn() -> bool + Send + 'static,
{
    let mut restart_count: u32 = 0;
    let mut last_status = HealthStatus::Unknown;

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

        match &status {
            HealthStatus::Healthy => {
                restart_count = 0;
            }
            HealthStatus::Unhealthy(_reason) if restart_count < config.max_restarts => {
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
            }
            HealthStatus::Unhealthy(_) => {
                warn!(
                    integration = integration_id,
                    "Max restarts exceeded, marking as Failed"
                );
                let _ = tx
                    .send(HealthEvent {
                        integration_id: integration_id.clone(),
                        status: HealthStatus::Unknown, // Failed state
                        restart_count,
                    })
                    .await;
                break;
            }
            _ => {}
        }

        last_status = status;
    }
}
