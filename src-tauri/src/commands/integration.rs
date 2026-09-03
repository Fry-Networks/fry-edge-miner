use crate::integrations::{HealthStatus, IntegrationStatus, LifecycleState};
use std::time::Duration;

/// Sentinel returned when Pawns.app is enabled without a recorded consent. The
/// frontend matches this exact string to open the consent dialog instead of
/// showing a raw error, so it must stay stable (mirrored in
/// `src/lib/consentDialog.ts`).
const PAWNS_CONSENT_REQUIRED: &str = "PAWNS_CONSENT_REQUIRED";

/// Upper bound on any single step of `toggle_integration` (the LAN pre-check,
/// install, or start). Belt-and-suspenders on top of the Bug 2 root-cause fix
/// in `supervisor::platform`/`mysterium_lan_check` (bounding the reap after
/// `kill()`, and moving the blocking LAN-process probe off the async worker
/// thread): even if some other future step in this path blocks
/// indefinitely, the toggle command itself must still resolve — a stuck
/// `invoke()` promise leaves the frontend with no error to show and no way
/// to recover a card stuck on `Installing` short of restarting the app.
/// Generous on purpose (SpaceAcres' own install waits up to 120s for its
/// installer, mirrored by callers who need more room than this step-level
/// guard), so this is deliberately a large ceiling, not a tight one.
const TOGGLE_STEP_TIMEOUT: Duration = Duration::from_secs(60);

/// Turn a `tokio::time::timeout` miss into the same shape of error string the
/// surrounding code already uses for a normal `Err`, so a stall and a real
/// failure look identical to the caller and to `last_integration_error`.
fn timeout_message(step: &str, id: &str) -> String {
    format!(
        "{id}: {step} did not finish within {}s and was aborted rather than left to hang. Try again.",
        TOGGLE_STEP_TIMEOUT.as_secs()
    )
}

#[tauri::command]
pub async fn get_integrations(
    state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<IntegrationStatus>, String> {
    // Snapshot registry metadata without holding the lock across any await point.
    let (entries, available) = {
        let reg = state.registry.lock().map_err(|e| e.to_string())?;
        // Share the reporter's denominator so the per-integration contribution
        // the UI shows matches what actually gets submitted.
        let available = reg.available_count();
        let entries: Vec<(String, String, bool, Option<String>, bool, Option<String>)> = reg
            .list()
            .iter()
            .map(|i| {
                (
                    i.id().to_string(),
                    i.display_name().to_string(),
                    reg.is_enabled(i.id()),
                    i.installed_version(),
                    i.requires_docker(),
                    i.check_requirements().err(),
                )
            })
            .collect();
        (entries, available)
    };

    // Read the most recent health check results written by the health loop in main.rs.
    let last = state.last_health.read().map_err(|e| e.to_string())?;
    let last_errors = state.last_integration_error.read().map_err(|e| e.to_string())?;

    let statuses = entries
        .into_iter()
        .map(|(id, display_name, enabled, version, requires_docker, unavailable_reason)| {
            let health = if enabled {
                last.get(&id)
                    .cloned()
                    .unwrap_or(HealthStatus::Starting)
            } else {
                last.get(&id)
                    .cloned()
                    .unwrap_or(HealthStatus::Stopped)
            };

            let lifecycle = if !enabled {
                LifecycleState::Disabled
            } else {
                match &health {
                    HealthStatus::Healthy => LifecycleState::Running,
                    HealthStatus::Unhealthy(_) => LifecycleState::Unhealthy,
                    HealthStatus::Installing => LifecycleState::Installing,
                    _ => LifecycleState::Starting,
                }
            };

            // Healthy-based so the UI matches what the PoC reporter actually
            // submits (reporter proportion counts Healthy only).
            let healthy = matches!(health, HealthStatus::Healthy);

            IntegrationStatus {
                id: id.clone(),
                display_name,
                enabled,
                health,
                lifecycle,
                version,
                poc_contribution: if enabled && healthy && available > 0 {
                    1.0 / available as f64
                } else {
                    0.0
                },
                tier: crate::integrations::tier_for(&id),
                requires_docker,
                error: last_errors.get(&id).and_then(|e| e.clone()),
                unavailable_reason,
            }
        })
        .collect();

    Ok(statuses)
}

#[tauri::command]
pub async fn install_integration(
    id: String,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    // Clone the integration Arc and release the registry lock before the long install operation.
    let integration = {
        let reg = state.registry.lock().map_err(|e| e.to_string())?;
        reg.get(&id)
            .ok_or_else(|| format!("Integration '{}' not found", id))?
    };

    integration.install().await.map_err(|e| e.to_string())?;
    tracing::info!(integration = id, "Integration installed");
    Ok(())
}

#[tauri::command]
pub async fn toggle_integration(
    id: String,
    enabled: bool,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    // Mutual exclusion: storj ↔ space_acres
    if enabled {
        let (mutually_exclusive_id, other_id) = match id.as_str() {
            "space_acres" => ("storj", "Storj"),
            "storj" => ("space_acres", "SpaceAcres"),
            _ => ("", ""),
        };

        if !mutually_exclusive_id.is_empty() {
            let is_other_enabled = {
                let reg = state.registry.lock().map_err(|e| e.to_string())?;
                reg.is_enabled(mutually_exclusive_id)
            };

            if is_other_enabled {
                return Err(format!(
                    "{} and {} are mutually exclusive. Disable {} first or choose a different integration.",
                    id, other_id, other_id
                ));
            }
        }

        // Check SpaceAcres eligibility
        if id == "space_acres" {
            let (eligible, reason) = crate::integrations::space_acres::SpaceAcresIntegration::check_eligibility().await;
            if !eligible {
                return Err(format!(
                    "{}. Try Storj instead.",
                    reason.unwrap_or_else(|| "SpaceAcres is not eligible on this device".to_string())
                ));
            }
        }

        // Check Mysterium LAN conflicts
        if id == "mysterium" {
            let cfg = state.config.get();
            if !cfg.myst_lan_override {
                let scan_result = tokio::time::timeout(
                    TOGGLE_STEP_TIMEOUT,
                    crate::integrations::mysterium_lan_check::scan_lan_conflict(),
                )
                .await
                .map_err(|_| timeout_message("LAN conflict check", &id))?
                .map_err(|e| e.to_string())?;
                if let Some(conflict) = scan_result {
                    return Err(format!(
                        "{}. Enable myst_lan_override in settings to proceed.",
                        conflict
                    ));
                }
            }
        }

        // Pawns.app routes other people's traffic through this connection, so
        // the CLI Addendum (§5.2–5.4) requires the device owner's explicit
        // consent before it may start. Refuse until one is on record; the UI
        // turns this sentinel into the consent dialog.
        if id == "pawns" && !crate::integrations::pawns::PawnsIntegration::user_consent() {
            return Err(PAWNS_CONSENT_REQUIRED.to_string());
        }
    }

    // Clone the integration Arc and release the registry lock before start/stop.
    let integration = {
        let reg = state.registry.lock().map_err(|e| e.to_string())?;
        reg.get(&id)
            .ok_or_else(|| format!("Integration '{}' not found", id))?
    };

    if enabled {
        // Hardware gate first: installing or starting an integration whose
        // minimums this machine cannot meet only produces a confusing partner
        // error later, so refuse up front with the specific reason.
        if let Err(reason) = integration.check_requirements() {
            if let Ok(mut errs) = state.last_integration_error.write() {
                errs.insert(id.clone(), Some(reason.clone()));
            }
            return Err(reason);
        }
        // Auto-install integrations that have not been deployed yet (e.g., Diiisco).
        if integration.installed_version().is_none() {
            match tokio::time::timeout(TOGGLE_STEP_TIMEOUT, integration.install()).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    let err_msg = e.to_string();
                    if let Ok(mut errs) = state.last_integration_error.write() {
                        errs.insert(id.clone(), Some(err_msg.clone()));
                    }
                    return Err(err_msg);
                }
                Err(_) => {
                    let err_msg = timeout_message("install", &id);
                    if let Ok(mut errs) = state.last_integration_error.write() {
                        errs.insert(id.clone(), Some(err_msg.clone()));
                    }
                    return Err(err_msg);
                }
            }
        }
        match tokio::time::timeout(TOGGLE_STEP_TIMEOUT, integration.start()).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let err_msg = e.to_string();
                if let Ok(mut errs) = state.last_integration_error.write() {
                    errs.insert(id.clone(), Some(err_msg.clone()));
                }
                return Err(err_msg);
            }
            Err(_) => {
                let err_msg = timeout_message("start", &id);
                if let Ok(mut errs) = state.last_integration_error.write() {
                    errs.insert(id.clone(), Some(err_msg.clone()));
                }
                return Err(err_msg);
            }
        }
        // Clear error on success
        if let Ok(mut errs) = state.last_integration_error.write() {
            errs.insert(id.clone(), None);
        }
    } else {
        if let Err(e) = integration.stop().await {
            let err_msg = e.to_string();
            if let Ok(mut errs) = state.last_integration_error.write() {
                errs.insert(id.clone(), Some(err_msg.clone()));
            }
            return Err(err_msg);
        }
        // Clear error on success
        if let Ok(mut errs) = state.last_integration_error.write() {
            errs.insert(id.clone(), None);
        }
    }

    // Only update state on success
    {
        let mut reg = state.registry.lock().map_err(|e| e.to_string())?;
        reg.set_enabled(&id, enabled);
    }

    // Persist config
    state
        .config
        .update(|cfg| {
            cfg.integrations_enabled.insert(id.clone(), enabled);
        })
        .map_err(|e| e.to_string())?;

    tracing::info!(integration = id, enabled = enabled, "Integration toggled");
    Ok(())
}

/// Force a clean reinstall of an integration whose installer state is stuck
/// (F2: uninstalled Olostep would neither reinstall nor surface why). Kills
/// the partner process, wipes every install artifact, then re-runs
/// install + start. Currently supported for Olostep (aem) only.
#[tauri::command]
pub async fn force_reinstall_integration(
    id: String,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    if id != "aem" {
        return Err(format!(
            "Force reinstall is not supported for integration '{}'",
            id
        ));
    }

    let integration = {
        let reg = state.registry.lock().map_err(|e| e.to_string())?;
        reg.get(&id)
            .ok_or_else(|| format!("Integration '{}' not found", id))?
    };

    tracing::info!(integration = id, "Force reinstall: cleaning previous install");
    tokio::task::block_in_place(crate::integrations::aem::AemIntegration::force_clean);

    if let Err(e) = integration.install().await {
        let err_msg = e.to_string();
        if let Ok(mut errs) = state.last_integration_error.write() {
            errs.insert(id.clone(), Some(err_msg.clone()));
        }
        return Err(err_msg);
    }
    if let Err(e) = integration.start().await {
        let err_msg = e.to_string();
        if let Ok(mut errs) = state.last_integration_error.write() {
            errs.insert(id.clone(), Some(err_msg.clone()));
        }
        return Err(err_msg);
    }

    // Reinstall implies the user wants it running — mirror the enable path.
    {
        let mut reg = state.registry.lock().map_err(|e| e.to_string())?;
        reg.set_enabled(&id, true);
    }
    state
        .config
        .update(|cfg| {
            cfg.integrations_enabled.insert(id.clone(), true);
        })
        .map_err(|e| e.to_string())?;
    if let Ok(mut errs) = state.last_integration_error.write() {
        errs.insert(id.clone(), None);
    }

    tracing::info!(integration = id, "Force reinstall complete");
    Ok(())
}

#[cfg(test)]
mod bug2_timeout_tests {
    use super::*;

    /// `toggle_integration` itself needs a full `tauri::State<AppState>`
    /// (registry, config, supervisor, api client, ...) to exercise end to
    /// end, which this crate has no test harness for anywhere today — the
    /// end-to-end proof for this file's change is the live gate (WP6's exact
    /// corrupted-binary repro, re-run against the fixed code). This test
    /// covers the one pure piece: the timeout-to-error-string mapping that
    /// `toggle_integration` now uses at all three bounded call sites, so a
    /// stall reads the same as a normal failure to the caller and to
    /// `last_integration_error`.
    #[test]
    fn timeout_message_names_the_step_the_integration_and_the_bound() {
        let msg = timeout_message("install", "mysterium");
        assert!(msg.contains("mysterium"), "{msg}");
        assert!(msg.contains("install"), "{msg}");
        assert!(msg.contains(&TOGGLE_STEP_TIMEOUT.as_secs().to_string()), "{msg}");
    }

    /// A genuinely-stalled future (never resolves) must still make
    /// `tokio::time::timeout` return within its bound — this is the same
    /// primitive `toggle_integration` wraps `scan_lan_conflict()`/
    /// `install()`/`start()` in. Uses a short local bound rather than the
    /// real 60s constant so the test stays fast.
    #[tokio::test]
    async fn a_future_that_never_resolves_is_still_bounded_by_timeout() {
        let never = std::future::pending::<()>();
        let started = std::time::Instant::now();
        let result = tokio::time::timeout(Duration::from_millis(50), never).await;
        assert!(result.is_err(), "a pending future must trip the timeout, not resolve");
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "timeout took {:?} — bound not enforced",
            started.elapsed()
        );
    }
}
