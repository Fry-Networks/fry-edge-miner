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
    /// Consecutive `Starting` checks tolerated before startup is called failed.
    /// 0 disables the timeout. Default 6 ≈ 3 minutes at a 30s interval.
    pub starting_timeout_ticks: u32,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(30),
            max_restarts: 3,
            backoff_base: Duration::from_secs(5),
            starting_timeout_ticks: 6,
        }
    }
}

/// What the loop should do about this tick's status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryAction {
    /// Healthy, disabled, or still legitimately starting.
    None,
    /// Unhealthy, or stopped while enabled.
    Restart,
    /// Startup never finished: report it, then recover like any other failure.
    StartupTimedOut,
}

/// Decide the action for one tick. Pure so the recovery rules are testable —
/// `health_check_loop` itself has no test coverage because it never returns.
///
/// `Installing` is deliberately NOT subject to the startup timeout: a Docker
/// pull or a partner installer can legitimately run for many minutes.
pub(crate) fn recovery_action(
    status: &HealthStatus,
    enabled: bool,
    consecutive_starting_ticks: u32,
    starting_timeout_ticks: u32,
) -> RecoveryAction {
    match status {
        HealthStatus::Unhealthy(_) => RecoveryAction::Restart,
        HealthStatus::Stopped if enabled => RecoveryAction::Restart,
        HealthStatus::Starting
            if enabled
                && starting_timeout_ticks > 0
                && consecutive_starting_ticks >= starting_timeout_ticks =>
        {
            RecoveryAction::StartupTimedOut
        }
        _ => RecoveryAction::None,
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
    let mut starting_ticks: u32 = 0;

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

        starting_ticks = if matches!(status, HealthStatus::Starting) {
            starting_ticks + 1
        } else {
            0
        };

        let action = recovery_action(
            &status,
            enabled_fn(),
            starting_ticks,
            config.starting_timeout_ticks,
        );

        if action == RecoveryAction::StartupTimedOut {
            // Say so plainly: a card stuck on amber "Starting" gave the user no
            // signal that anything was wrong, while the integration earned zero.
            let secs = (config.check_interval * starting_ticks).as_secs();
            warn!(
                integration = integration_id,
                seconds = secs,
                "Startup did not complete — treating as failed"
            );
            let _ = tx
                .send(HealthEvent {
                    integration_id: integration_id.clone(),
                    status: HealthStatus::Unhealthy(format!(
                        "Did not finish starting within {secs}s — retrying"
                    )),
                    restart_count,
                })
                .await;
            starting_ticks = 0;
        }

        let needs_restart = action != RecoveryAction::None;

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

#[cfg(test)]
mod recovery_tests {
    use super::*;

    const LIMIT: u32 = 6;

    #[test]
    fn an_unhealthy_integration_is_restarted() {
        assert_eq!(
            recovery_action(&HealthStatus::Unhealthy("boom".into()), true, 0, LIMIT),
            RecoveryAction::Restart
        );
    }

    #[test]
    fn a_stopped_integration_is_restarted_only_while_enabled() {
        assert_eq!(
            recovery_action(&HealthStatus::Stopped, true, 0, LIMIT),
            RecoveryAction::Restart
        );
        assert_eq!(
            recovery_action(&HealthStatus::Stopped, false, 0, LIMIT),
            RecoveryAction::None
        );
    }

    #[test]
    fn a_healthy_integration_is_left_alone() {
        assert_eq!(
            recovery_action(&HealthStatus::Healthy, true, 0, LIMIT),
            RecoveryAction::None
        );
    }

    #[test]
    fn starting_is_tolerated_up_to_the_limit() {
        for ticks in 0..LIMIT {
            assert_eq!(
                recovery_action(&HealthStatus::Starting, true, ticks, LIMIT),
                RecoveryAction::None,
                "tick {ticks} should still be waiting"
            );
        }
    }

    /// The gap this closes: `Starting` used to fall through to "do nothing", so
    /// an integration that never finished starting (Mysterium without its QUIC
    /// marker, Pawns before its agent authenticates) sat amber forever,
    /// contributing nothing to rewards and never being retried.
    #[test]
    fn startup_that_never_finishes_is_eventually_failed() {
        assert_eq!(
            recovery_action(&HealthStatus::Starting, true, LIMIT, LIMIT),
            RecoveryAction::StartupTimedOut
        );
        assert_eq!(
            recovery_action(&HealthStatus::Starting, true, LIMIT + 3, LIMIT),
            RecoveryAction::StartupTimedOut
        );
    }

    #[test]
    fn a_disabled_integration_never_times_out() {
        assert_eq!(
            recovery_action(&HealthStatus::Starting, false, LIMIT + 10, LIMIT),
            RecoveryAction::None
        );
    }

    /// A long Docker pull or partner installer must not be killed for being slow.
    #[test]
    fn installing_is_exempt_from_the_startup_timeout() {
        assert_eq!(
            recovery_action(&HealthStatus::Installing, true, LIMIT + 10, LIMIT),
            RecoveryAction::None
        );
    }

    #[test]
    fn a_zero_limit_disables_the_timeout() {
        assert_eq!(
            recovery_action(&HealthStatus::Starting, true, 9_999, 0),
            RecoveryAction::None
        );
    }

    #[test]
    fn the_default_startup_budget_is_a_conservative_three_minutes() {
        let c = HealthCheckConfig::default();
        assert_eq!(c.starting_timeout_ticks, 6);
        assert_eq!(c.check_interval * c.starting_timeout_ticks, Duration::from_secs(180));
    }
}
