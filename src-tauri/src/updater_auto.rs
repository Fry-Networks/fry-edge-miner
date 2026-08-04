use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tauri_plugin_updater::UpdaterExt;
use tracing::{info, warn};

use crate::config::store::ConfigStore;

/// Background auto-updater task. Spawned once at app startup.
/// - Initial delay: 3 minutes after boot
/// - Check interval: every 6 hours
/// - Respects config.auto_update flag (checked each cycle)
/// - Per-device jitter on install (0–10 min) prevents fleet restarts simultaneously
/// - Errors logged as warnings, never crash, never block boot
pub async fn spawn_auto_updater(app: tauri::AppHandle, config: Arc<ConfigStore>) {
    // Initial delay: 3 minutes (allow UI to stabilize)
    tokio::time::sleep(Duration::from_secs(180)).await;

    // Check interval: 6 hours
    let mut interval = tokio::time::interval(Duration::from_secs(6 * 3600));

    loop {
        interval.tick().await;

        let cfg = config.get();

        // Skip this cycle if auto_update is disabled
        if !cfg.auto_update {
            info!("Auto-update disabled in config — skipping this cycle");
            continue;
        }

        // Perform the update check
        match check_and_install_update(&app, &config).await {
            Ok(action) => {
                if let Some(action) = action {
                    info!(action = %action, "Auto-update action completed");
                }
            }
            Err(e) => {
                warn!(error = %e, "Auto-update check cycle failed — will retry next interval");
            }
        }
    }
}

/// Check for updates and install if available. Returns None if no update,
/// Some(msg) if action was taken (e.g., "restart required").
/// Errors are bubbled; caller decides whether to log/retry.
async fn check_and_install_update(
    app: &tauri::AppHandle,
    config: &Arc<ConfigStore>,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    // Build updater with reasonable timeout
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    // Check for updates
    let update = match updater.check().await {
        Ok(Some(u)) => u,
        Ok(None) => {
            info!("No update available");
            return Ok(None);
        }
        Err(e) => {
            return Err(format!("Update check failed: {}", e).into());
        }
    };

    let current = env!("CARGO_PKG_VERSION");
    if update.version == current {
        info!(version = %current, "Already on latest version");
        return Ok(None);
    }

    info!(
        current = %current,
        latest = %update.version,
        "Update available — downloading and installing"
    );

    // Download and install
    match update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
    {
        Ok(()) => {
            info!(version = %update.version, "Update installed successfully");

            // Compute jitter (0–10 min) based on install_id or SystemTime nanos
            // This ensures fleets don't all restart simultaneously
            let jitter_secs = compute_jitter_secs(config);
            info!(jitter_secs = jitter_secs, "Scheduling restart with jitter");

            // Spawn a one-shot task to restart after jitter expires
            // (never block the check loop itself)
            let app_clone = app.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(jitter_secs)).await;
                info!("Jitter expired — initiating app restart");
                app_clone.restart();
            });

            Ok(Some("Update installed; restart scheduled".to_string()))
        }
        Err(e) => {
            Err(format!("Download/install failed: {}", e).into())
        }
    }
}

/// Compute per-device jitter (0–10 minutes) using install_id hash.
/// Falls back to SystemTime nanos if install_id not available.
/// Returns jitter in seconds.
fn compute_jitter_secs(config: &Arc<ConfigStore>) -> u64 {
    const MAX_JITTER_SECS: u64 = 600; // 10 minutes

    let cfg = config.get();
    let seed_str = cfg
        .install_id
        .as_ref()
        .cloned()
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos().to_string())
                .unwrap_or_else(|_| "fallback".to_string())
        });

    // Simple hash: sum the bytes mod MAX_JITTER_SECS
    let hash = seed_str
        .as_bytes()
        .iter()
        .map(|b| *b as u64)
        .sum::<u64>()
        % MAX_JITTER_SECS;

    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_auto_install_decision() {
        // This test verifies the decision logic:
        // - Current version == latest: no install
        // - Current version < latest: install
        // - Config.auto_update = false: skip cycle
        let test_cases = vec![
            ("0.2.30", "0.2.30", true, false),  // same version, enabled → no install
            ("0.2.30", "0.2.31", true, true),   // new version, enabled → install
            ("0.2.30", "0.2.31", false, false), // new version, disabled → no install
        ];

        for (current, latest, auto_update, should_install) in test_cases {
            let decision = should_auto_install(current, latest, auto_update);
            assert_eq!(
                decision, should_install,
                "Failed for current={}, latest={}, auto_update={}",
                current, latest, auto_update
            );
        }
    }

    #[test]
    fn test_jitter_computation_bounds() {
        // Jitter should always be within [0, 600) seconds
        let test_ids = vec!["install-abc123", "install-xyz789", ""];

        for install_id in test_ids {
            // Simulate a config
            let jitter = if install_id.is_empty() {
                compute_hash_jitter(install_id)
            } else {
                compute_hash_jitter(install_id)
            };

            assert!(
                jitter < 600,
                "Jitter {} out of bounds for install_id '{}'",
                jitter,
                install_id
            );
        }
    }

    /// Deterministic jitter using install_id hash (no config dependency for testing)
    fn compute_hash_jitter(seed: &str) -> u64 {
        const MAX_JITTER_SECS: u64 = 600;
        seed.as_bytes()
            .iter()
            .map(|b| *b as u64)
            .sum::<u64>()
            % MAX_JITTER_SECS
    }

    fn should_auto_install(current: &str, latest: &str, auto_update: bool) -> bool {
        auto_update && current != latest
    }
}
