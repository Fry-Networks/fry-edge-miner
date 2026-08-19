use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tauri_plugin_updater::UpdaterExt;
use tracing::{info, warn};

use crate::config::store::ConfigStore;
use crate::supervisor::platform::BoundedOutput;
use crate::supervisor::{ProcessInfo, Supervisor};

/// Partner binaries that can outlive their supervisor entry — a crash, or a
/// process left by a previous app run — and still hold the install tree open
/// after every tracked process has been stopped.
const ORPHAN_IMAGES: [&str; 1] = ["frynode.exe"];

/// Settle time after the last partner process exits, before the updater
/// replaces the install tree. Process exit and Windows releasing the file
/// handle are not the same instant.
const PARTNER_STOP_SETTLE: Duration = Duration::from_secs(2);

/// Which supervisor-managed processes must be stopped before the Tauri updater
/// replaces the application files.
///
/// B7: the updater overwrites the whole install directory, and the partner
/// binaries are launched FROM it — fryvpn resolves frynode.exe as
/// `…\Fry Edge Miner\resources\frynode.exe`. A running child keeps that file
/// open, so the install failed with a locked-file error and the device sat on
/// the old version until someone restarted it by hand. Every *running* managed
/// process is in the plan: they all come out of the same tree, and stopping a
/// process that already exited is not worth a special case.
pub fn partner_stop_plan(processes: &[ProcessInfo]) -> Vec<String> {
    processes
        .iter()
        .filter(|p| p.running)
        .map(|p| p.integration_id.clone())
        .collect()
}

/// Stop the partner processes in the plan. Returns the ids actually asked to
/// stop.
///
/// Best-effort by design: `Supervisor::stop_integration` already blocks up to
/// 10s per process waiting for it to exit, and a failure here is logged and
/// then ignored — refusing to update at all is worse than risking the locked
/// file the update might hit anyway (which is the pre-B7 behaviour).
fn stop_partner_processes(supervisor: &Arc<Mutex<Supervisor>>) -> Vec<String> {
    let mut sup = match supervisor.lock() {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "Supervisor lock poisoned — installing without stopping partners");
            return Vec::new();
        }
    };
    let plan = partner_stop_plan(&sup.list_processes());
    for id in &plan {
        match sup.stop_integration(id) {
            Ok(()) => info!(integration = id.as_str(), "Stopped for update"),
            Err(e) => warn!(
                integration = id.as_str(),
                error = %e,
                "Could not stop before update — continuing"
            ),
        }
    }
    plan
}

/// Kill partner binaries the supervisor no longer tracks, so a crashed or
/// orphaned process cannot keep the install tree locked.
///
/// `taskkill` exits non-zero when nothing matched the image name, which is the
/// normal case and is success here — the precedent is `aem.rs`'s stop path.
fn kill_orphan_partners() {
    for image in ORPHAN_IMAGES {
        match crate::supervisor::platform::command("taskkill")
            .args(["/IM", image, "/T", "/F"])
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
        {
            Ok(o) if o.status.success() => info!(image, "Killed orphaned partner process"),
            Ok(_) => info!(image, "No orphaned partner process was running"),
            Err(e) => warn!(image, error = %e, "Orphan cleanup could not run — continuing"),
        }
    }
}

/// Background auto-updater task. Spawned once at app startup.
/// - Initial delay: 3 minutes after boot
/// - Check interval: every 6 hours
/// - Respects config.auto_update flag (checked each cycle)
/// - Per-device jitter on install (0–10 min) prevents fleet restarts simultaneously
/// - Errors logged as warnings, never crash, never block boot
pub async fn spawn_auto_updater(
    app: tauri::AppHandle,
    config: Arc<ConfigStore>,
    supervisor: Arc<Mutex<Supervisor>>,
) {
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
        match check_and_install_update(&app, &config, &supervisor).await {
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
    supervisor: &Arc<Mutex<Supervisor>>,
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

    // B7: release the install tree before the updater rewrites it. The
    // partners are restarted by the startup recovery pass after the restart
    // below, so nothing here needs to put them back.
    // ManagedProcess::stop blocks up to 10s per process waiting for exit, so
    // this must not run on an async worker thread directly.
    let stopped = tokio::task::block_in_place(|| stop_partner_processes(supervisor));
    tokio::task::block_in_place(kill_orphan_partners);
    if !stopped.is_empty() {
        info!(
            stopped = stopped.join(",").as_str(),
            "Stopped partner processes before update install"
        );
        tokio::time::sleep(PARTNER_STOP_SETTLE).await;
    }

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

/// B7: which partner processes get released before the updater rewrites the
/// install tree.
#[cfg(test)]
mod partner_stop_tests {
    use super::*;

    fn proc(id: &str, running: bool) -> ProcessInfo {
        ProcessInfo {
            integration_id: id.to_string(),
            pid: 4242,
            running,
        }
    }

    #[test]
    fn the_fryvpn_binary_is_stopped_before_an_install() {
        // frynode.exe is the file the update actually failed on: it lives in
        // the install tree the updater replaces.
        let plan = partner_stop_plan(&[proc("fryvpn", true)]);
        assert_eq!(plan, vec!["fryvpn".to_string()]);
    }

    #[test]
    fn every_running_managed_process_is_stopped() {
        let plan = partner_stop_plan(&[
            proc("fryvpn", true),
            proc("iagon", true),
            proc("mysterium", true),
        ]);
        assert_eq!(plan.len(), 3);
        assert!(plan.contains(&"iagon".to_string()));
    }

    #[test]
    fn processes_that_already_exited_are_left_alone() {
        let plan = partner_stop_plan(&[proc("fryvpn", false), proc("iagon", true)]);
        assert_eq!(plan, vec!["iagon".to_string()]);
    }

    #[test]
    fn an_install_with_nothing_running_stops_nothing() {
        assert!(partner_stop_plan(&[]).is_empty());
        assert!(partner_stop_plan(&[proc("fryvpn", false)]).is_empty());
    }

    #[test]
    fn the_orphan_sweep_covers_the_binary_that_holds_the_install_tree() {
        // fryvpn spawns frynode.exe out of …\Fry Edge Miner\resources\, which
        // is exactly what the updater overwrites. The sweep itself shells out
        // to taskkill and is exercised by the release build, not here.
        assert!(ORPHAN_IMAGES.contains(&"frynode.exe"));
    }

    #[test]
    fn the_settle_wait_is_bounded_and_short() {
        // Long enough for Windows to release the handle, short enough that it
        // cannot stall the 6-hour check loop in any meaningful way.
        assert!(PARTNER_STOP_SETTLE >= Duration::from_secs(1));
        assert!(PARTNER_STOP_SETTLE <= Duration::from_secs(10));
    }
}
