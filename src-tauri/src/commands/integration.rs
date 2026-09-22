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

/// An INSTALL is not a step of that shape. It downloads a partner release over
/// whatever link the user has, extracts it, and may run an elevated redist
/// installer with its own 600 s budget — while the HTTP client it downloads
/// through allows 300 s per request. Bounding all of that at 60 s meant a slow
/// link or a slow disk had the install cancelled mid-flight and it could never
/// complete FROM THE CARD, even though the unbounded boot-time path installed
/// the very same partner successfully.
const INSTALL_STEP_TIMEOUT: Duration = crate::supervisor::platform::LONG_TIMEOUT;

/// Turn a `tokio::time::timeout` miss into the same shape of error string the
/// surrounding code already uses for a normal `Err`, so a stall and a real
/// failure look identical to the caller and to `last_integration_error`.
fn timeout_message(step: &str, id: &str) -> String {
    timeout_message_with(step, id, TOGGLE_STEP_TIMEOUT)
}

/// The same message against an explicit bound, so a step with its own deadline
/// reports the deadline it was actually held to.
fn timeout_message_with(step: &str, id: &str, bound: Duration) -> String {
    format!(
        "{id}: {step} did not finish within {}s and was aborted rather than left to hang. Try again.",
        bound.as_secs()
    )
}

/// One integration's raw row before it is turned into an `IntegrationStatus`:
/// (id, display name, enabled, version, requires_docker, unavailable_reason).
/// Named because clippy::type_complexity flags the bare tuple.
type IntegrationEntry = (String, String, bool, Option<String>, bool, Option<String>);

#[tauri::command]
pub async fn get_integrations(
    state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<IntegrationStatus>, String> {
    // Snapshot registry metadata without holding the lock across any await
    // point — and, since B21, without holding it across the slow per-integration
    // probes either. `installed_version()` shells out to PowerShell up to three
    // times for SpaceAcres, each bounded at 20 s, and this runs on a 30 s poll;
    // holding the registry mutex across that stalled the user's own click,
    // because toggle_integration needs the same mutex.
    //
    // Only `is_enabled` and `available_count` actually need the lock.
    let (handles, enabled_flags, available) = {
        let reg = state.registry.lock().map_err(|e| e.to_string())?;
        // Share the reporter's denominator so the per-integration contribution
        // the UI shows matches what actually gets submitted.
        let available = reg.available_count();
        let handles = reg.list();
        let enabled_flags: std::collections::HashMap<String, bool> = handles
            .iter()
            .map(|i| (i.id().to_string(), reg.is_enabled(i.id())))
            .collect();
        (handles, enabled_flags, available)
    };

    let entries: Vec<IntegrationEntry> = handles
        .iter()
        .map(|i| {
            (
                i.id().to_string(),
                i.display_name().to_string(),
                enabled_flags.get(i.id()).copied().unwrap_or(false),
                i.installed_version(),
                i.requires_docker(),
                i.check_requirements().err(),
            )
        })
        .collect();

    // Read the most recent health check results written by the health loop in main.rs.
    let last = state.last_health.read().map_err(|e| e.to_string())?;
    let last_errors = state
        .last_integration_error
        .read()
        .map_err(|e| e.to_string())?;
    // B3 defect 4: an elevation FEM suppressed, or one the user declined, has
    // no other way to reach the card — every elevation site is `warn!`-only and
    // none of them writes `last_integration_error`. Merged here (a snapshot, so
    // no lock is held across the map) and only where there is no more specific
    // error already, so a real start failure is never masked by it.
    let elevation_blocks = crate::elevation_gate::blocked_reasons();
    // B14 D5: enables that are still running, so the poll reinforces the
    // frontend's spinner instead of overwriting it with "Not installed".
    let pending_enables = state
        .pending_enable
        .read()
        .map(|p| p.clone())
        .unwrap_or_default();

    let statuses = entries
        .into_iter()
        .map(
            |(id, display_name, enabled, version, requires_docker, unavailable_reason)| {
                let observed = if enabled {
                    last.get(&id).cloned().unwrap_or(HealthStatus::Starting)
                } else {
                    last.get(&id).cloned().unwrap_or(HealthStatus::Stopped)
                };

                let (enabled, health, lifecycle) =
                    pending_view(enabled, pending_enables.contains(&id), observed);

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
                    error: last_errors
                        .get(&id)
                        .and_then(|e| e.clone())
                        .or_else(|| elevation_blocks.get(&id).cloned()),
                    unavailable_reason,
                }
            },
        )
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

/// Record why an enable attempt failed, so the card has something to show.
///
/// Six enable-path exits returned an Err that reached the toast and nothing
/// else: `last_integration_error` was never written, so the per-card error
/// slot stayed empty and - once the next poll cleared the toast - the toggle
/// simply appeared to flip itself back off with no explanation anywhere.
fn record_enable_error(state: &tauri::State<'_, crate::AppState>, id: &str, msg: &str) {
    if let Ok(mut errs) = state.last_integration_error.write() {
        errs.insert(id.to_string(), Some(msg.to_string()));
    }
}

/// PURE: what the card should show while an enable is still in flight.
///
/// B14 "auto-install not firing": nothing in production ever wrote
/// `HealthStatus::Installing` into `last_health`, so the backend could never
/// report Installing at all — the arm existed only as a consumer. The ungated
/// 30 s poll therefore overwrote the frontend's optimistic spinner with
/// "Not installed" and a toggle flipped back OFF, while the install was still
/// running. Reporting the pending enable makes the poll REINFORCE the spinner
/// instead of fighting it.
///
/// `healthy` stays false for the whole window, so `poc_contribution` remains 0
/// and the reward model is untouched.
pub(crate) fn pending_view(
    enabled: bool,
    pending: bool,
    health: HealthStatus,
) -> (bool, HealthStatus, LifecycleState) {
    if pending {
        return (true, HealthStatus::Installing, LifecycleState::Installing);
    }
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
    (enabled, health, lifecycle)
}

/// Marks an integration as mid-enable for as long as it is alive.
///
/// RAII rather than a manual remove: `toggle_integration` has a dozen early
/// `return Err(...)` paths, and any one of them leaking an entry would pin
/// that card on "Installing" forever.
struct PendingGuard<'a> {
    set: &'a std::sync::RwLock<std::collections::HashSet<String>>,
    id: String,
}

impl<'a> PendingGuard<'a> {
    fn new(set: &'a std::sync::RwLock<std::collections::HashSet<String>>, id: &str) -> Self {
        if let Ok(mut pending) = set.write() {
            pending.insert(id.to_string());
        }
        Self {
            set,
            id: id.to_string(),
        }
    }
}

impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.set.write() {
            pending.remove(&self.id);
        }
    }
}

/// PURE: the message for two integrations that cannot run at once.
pub(crate) fn mutual_exclusion_conflict(
    id: &str,
    other_id: &str,
    other_enabled: bool,
) -> Option<String> {
    if other_id.is_empty() || !other_enabled {
        return None;
    }
    Some(format!(
        "{} and {} are mutually exclusive. Disable {} first or choose a different integration.",
        id, other_id, other_id
    ))
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

            if let Some(msg) = mutual_exclusion_conflict(&id, other_id, is_other_enabled) {
                record_enable_error(&state, &id, &msg);
                return Err(msg);
            }
        }

        // Check SpaceAcres eligibility
        if id == "space_acres" {
            let (eligible, reason) =
                crate::integrations::space_acres::SpaceAcresIntegration::check_eligibility().await;
            if !eligible {
                let msg = format!(
                    "{}. Try Storj instead.",
                    reason
                        .unwrap_or_else(|| "SpaceAcres is not eligible on this device".to_string())
                );
                record_enable_error(&state, &id, &msg);
                return Err(msg);
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
                .map_err(|_| {
                    let msg = timeout_message("LAN conflict check", &id);
                    record_enable_error(&state, &id, &msg);
                    msg
                })?
                .map_err(|e| {
                    let msg = e.to_string();
                    record_enable_error(&state, &id, &msg);
                    msg
                })?;
                if let Some(conflict) = scan_result {
                    let msg = format!(
                        "{}. Enable myst_lan_override in settings to proceed.",
                        conflict
                    );
                    record_enable_error(&state, &id, &msg);
                    return Err(msg);
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

    // B14 D5: from here on an enable is genuinely in flight, so the card must
    // say so. Armed AFTER the pre-checks, so a refusal never shows a spinner,
    // and RAII so no early return can leave the card stuck on "Installing".
    let _pending = if enabled {
        Some(PendingGuard::new(&state.pending_enable, &id))
    } else {
        None
    };

    // Clone the integration Arc and release the registry lock before start/stop.
    let integration = {
        let reg = state.registry.lock().map_err(|e| e.to_string())?;
        reg.get(&id).ok_or_else(|| {
            let msg = format!("Integration '{}' not found", id);
            record_enable_error(&state, &id, &msg);
            msg
        })?
    };

    if enabled {
        // Hardware gate first: installing or starting an integration whose
        // minimums this machine cannot meet only produces a confusing partner
        // error later, so refuse up front with the specific reason.
        if let Err(reason) = integration.check_requirements() {
            record_enable_error(&state, &id, &reason);
            return Err(reason);
        }
        // B3 (D-02): toggling ON is the retry gesture for a suppressed or
        // declined elevation — the ratified design ships no separate Retry
        // button. Clear any block recorded for this integration so the card
        // reflects what THIS attempt does rather than what the last one did.
        crate::elevation_gate::clear_blocked(&id);

        // Auto-install integrations that have not been deployed yet (e.g., Diiisco).
        if integration.installed_version().is_none() {
            match tokio::time::timeout(INSTALL_STEP_TIMEOUT, integration.install()).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    let err_msg = e.to_string();
                    if let Ok(mut errs) = state.last_integration_error.write() {
                        errs.insert(id.clone(), Some(err_msg.clone()));
                    }
                    return Err(err_msg);
                }
                Err(_) => {
                    let err_msg = timeout_message_with("install", &id, INSTALL_STEP_TIMEOUT);
                    if let Ok(mut errs) = state.last_integration_error.write() {
                        errs.insert(id.clone(), Some(err_msg.clone()));
                    }
                    return Err(err_msg);
                }
            }
        }
        // B3: this is the one path a human actually clicked, so it is the one
        // path allowed to raise a UAC prompt. Every other caller of start()
        // (boot auto-start, supervisor restart, the Docker watcher) keeps
        // getting ElevationTrigger::Automatic, which the gate refuses before
        // any prompt appears.
        match tokio::time::timeout(TOGGLE_STEP_TIMEOUT, integration.start_for_user()).await {
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
        // B9: the USER turning fryDVPN off must deregister the node, not just
        // kill it. Every other integration's default is still `stop()`.
        if let Err(e) = integration.stop_for_disable().await {
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

    tracing::info!(
        integration = id,
        "Force reinstall: cleaning previous install"
    );
    tokio::task::block_in_place(crate::integrations::aem::AemIntegration::force_clean);

    if let Err(e) = integration.install().await {
        let err_msg = e.to_string();
        if let Ok(mut errs) = state.last_integration_error.write() {
            errs.insert(id.clone(), Some(err_msg.clone()));
        }
        return Err(err_msg);
    }
    // Force-reinstall is a button the user pressed, so it carries the same
    // elevation authority as the toggle.
    if let Err(e) = integration.start_for_user().await {
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
        assert!(
            msg.contains(&TOGGLE_STEP_TIMEOUT.as_secs().to_string()),
            "{msg}"
        );
    }

    /// B13: the outer bound on an install was ten times SMALLER than budgets
    /// the install itself contains, so a slow install was cancelled mid-flight
    /// and could never complete from the card.
    #[test]
    fn the_install_step_bound_exceeds_every_budget_install_itself_contains() {
        assert!(
            INSTALL_STEP_TIMEOUT > TOGGLE_STEP_TIMEOUT,
            "an install needs more room than a generic toggle step"
        );
        assert!(
            INSTALL_STEP_TIMEOUT > crate::integrations::titan::VC_REDIST_INSTALL_TIMEOUT,
            "the outer bound must outlast the elevated redist install it wraps"
        );
        assert!(
            INSTALL_STEP_TIMEOUT >= Duration::from_secs(300),
            "the outer bound must outlast the HTTP client timeout downloads use"
        );
    }

    /// A timeout message that names the wrong number tells the user to wait
    /// for a deadline that was never applied.
    #[test]
    fn the_install_timeout_message_names_the_install_bound_not_the_toggle_bound() {
        let msg = timeout_message_with("install", "titan", INSTALL_STEP_TIMEOUT);
        assert!(
            msg.contains(&INSTALL_STEP_TIMEOUT.as_secs().to_string()),
            "{msg}"
        );
        assert!(
            !msg.contains(&format!("within {}s", TOGGLE_STEP_TIMEOUT.as_secs())),
            "{msg}"
        );
    }

    /// The delegating wrapper must keep producing exactly what it did before.
    #[test]
    fn timeout_message_still_reports_the_toggle_bound() {
        assert_eq!(
            timeout_message("start", "pawns"),
            timeout_message_with("start", "pawns", TOGGLE_STEP_TIMEOUT)
        );
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
        assert!(
            result.is_err(),
            "a pending future must trip the timeout, not resolve"
        );
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "timeout took {:?} — bound not enforced",
            started.elapsed()
        );
    }
}

/// B14 — "silent toggle revert".
///
/// Six enable-path exits returned an Err that reached the toast and nothing
/// else. `last_integration_error` was never written, so the per-card error
/// slot stayed empty; once the next 30 s poll cleared the toast, the toggle
/// looked like it had flipped itself back off for no reason.
#[cfg(test)]
mod b14_enable_error_tests {
    use super::*;

    #[test]
    fn mutually_exclusive_integrations_are_named_in_the_message() {
        let msg = mutual_exclusion_conflict("space_acres", "Storj", true)
            .expect("an enabled counterpart must block the toggle");
        assert!(msg.contains("mutually exclusive"), "{msg}");
        assert!(msg.contains("space_acres"), "{msg}");
        assert!(msg.contains("Storj"), "{msg}");
    }

    #[test]
    fn there_is_no_conflict_when_the_counterpart_is_off_or_absent() {
        assert!(mutual_exclusion_conflict("space_acres", "Storj", false).is_none());
        assert!(mutual_exclusion_conflict("mysterium", "", true).is_none());
        assert!(mutual_exclusion_conflict("mysterium", "", false).is_none());
    }

    /// The exact string must not drift: the frontend shows it verbatim.
    #[test]
    fn the_conflict_message_is_byte_identical_to_what_shipped() {
        assert_eq!(
            mutual_exclusion_conflict("storj", "SpaceAcres", true).unwrap(),
            "storj and SpaceAcres are mutually exclusive. Disable SpaceAcres first or choose a different integration."
        );
    }

    /// B14 D5: nothing in production ever wrote HealthStatus::Installing, so
    /// the backend could not report an install in progress at all and the
    /// 30 s poll overwrote the frontend's spinner with "Not installed" while
    /// the install was still running.
    #[test]
    fn a_pending_enable_is_reported_as_installing() {
        assert_eq!(
            pending_view(false, true, HealthStatus::Stopped),
            (true, HealthStatus::Installing, LifecycleState::Installing),
            "an enable in flight must read as Installing even before set_enabled runs"
        );
    }

    #[test]
    fn a_settled_integration_is_unaffected() {
        assert_eq!(
            pending_view(true, false, HealthStatus::Healthy),
            (true, HealthStatus::Healthy, LifecycleState::Running)
        );
        assert_eq!(
            pending_view(false, false, HealthStatus::Stopped),
            (false, HealthStatus::Stopped, LifecycleState::Disabled)
        );
        assert_eq!(
            pending_view(true, false, HealthStatus::Unhealthy("boom".to_string())),
            (
                true,
                HealthStatus::Unhealthy("boom".to_string()),
                LifecycleState::Unhealthy
            )
        );
    }

    /// The window must not pay out: `healthy` stays false throughout, so
    /// poc_contribution is 0 and the reward model is untouched.
    #[test]
    fn a_pending_enable_is_never_counted_as_healthy() {
        let (_, health, _) = pending_view(false, true, HealthStatus::Healthy);
        assert!(
            !matches!(health, HealthStatus::Healthy),
            "an install in flight must not contribute to PoC"
        );
    }

    /// Every early return in toggle_integration must release the marker, or
    /// the card pins on "Installing" forever.
    #[test]
    fn the_pending_marker_is_released_even_on_an_early_return() {
        let set = std::sync::RwLock::new(std::collections::HashSet::new());
        {
            let _guard = PendingGuard::new(&set, "titan");
            assert!(set.read().unwrap().contains("titan"));
        }
        assert!(
            set.read().unwrap().is_empty(),
            "the guard must clear the marker on every path out"
        );
    }

    /// Needles are assembled at runtime so this guard cannot be satisfied by
    /// its own source text.
    fn toggle_body() -> String {
        let src = include_str!("integration.rs");
        let start = src
            .find(&format!("pub async fn toggle{}", "_integration("))
            .expect("toggle_integration must exist");
        let end = src[start..]
            .find(&format!("pub async fn force{}", "_reinstall_integration("))
            .map(|i| start + i)
            .unwrap_or(src.len());
        src[start..end].to_string()
    }

    /// Every early exit from the enable path must leave something on the card
    /// — except the Pawns consent sentinel, which the UI turns into a dialog
    /// and which would read as raw noise if it were painted as an error.
    #[test]
    fn no_enable_path_exit_is_silent() {
        let body = toggle_body();
        let record = format!("record_enable{}", "_error(");
        let legacy = format!("last_integration{}", "_error.write()");
        let sentinel = format!("PAWNS_CONSENT{}", "_REQUIRED");
        let needle = format!("return Err{}", "(");

        let lines: Vec<&str> = body.lines().collect();
        let mut silent = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if !line.contains(&needle) || line.contains(&sentinel) {
                continue;
            }
            let lo = i.saturating_sub(4);
            let window = lines[lo..i].join("\n");
            if !window.contains(&record) && !window.contains(&legacy) {
                silent.push(*line);
            }
        }
        assert!(
            silent.is_empty(),
            "these enable-path exits record nothing for the card: {silent:?}"
        );
    }

    /// Locks in the deliberate exception.
    #[test]
    fn the_pawns_consent_sentinel_is_deliberately_not_recorded_as_an_error() {
        let body = toggle_body();
        let sentinel = format!("PAWNS_CONSENT{}", "_REQUIRED");
        let record = format!("record_enable{}", "_error(");

        let line = body
            .lines()
            .position(|l| l.contains(&sentinel) && l.contains("return"))
            .expect("the sentinel exit must still exist");
        let lines: Vec<&str> = body.lines().collect();
        let window = lines[line.saturating_sub(3)..line].join("\n");
        assert!(
            !window.contains(&record),
            "recording the sentinel would paint it on the card and duplicate the consent dialog"
        );
    }
}

/// B21 — a 30 s poll must not stall the user's click.
///
/// `get_integrations` held the registry mutex across `installed_version()`,
/// which for SpaceAcres shells out to PowerShell up to three times, each
/// bounded at 20 s. `toggle_integration` needs the same mutex, so a poll in
/// that state blocked the toggle the user had just flipped.
#[cfg(test)]
mod b21_poll_lock_tests {
    /// B21 D5: `get_integrations` held the registry mutex across
    /// `installed_version()`, which for SpaceAcres shells out to PowerShell up
    /// to three times at 20 s each — on a 30 s poll. `toggle_integration` needs
    /// the same mutex, so a poll in that state blocked the click the user had
    /// just made.
    ///
    /// This reads the REAL function out of this file. An earlier version of
    /// this test built its own `Mutex` and worker thread and measured those,
    /// which proved only that `std::sync::Mutex` releases a dropped guard —
    /// true on every commit this repo has ever had, and therefore unable to
    /// tell the fixed function from the broken one. Needles are assembled at
    /// runtime so the guard cannot be satisfied by its own source text.
    #[test]
    fn the_registry_guard_is_released_before_the_slow_per_integration_probes() {
        let src = include_str!("integration.rs");

        let fn_at = src
            .find(&format!("pub async fn get{}", "_integrations("))
            .expect("get_integrations must exist");
        let fn_end = src[fn_at..]
            .find("// Read the most recent health check")
            .map(|e| fn_at + e)
            .expect("the health-check read marks the end of the snapshot phase");
        let phase = &src[fn_at..fn_end];

        let lock_at = phase
            .find(&format!("state.registry.{}()", "lock"))
            .expect("the registry lock must be taken in get_integrations");
        let snapshot_end = phase[lock_at..]
            .find("\n    };")
            .map(|e| lock_at + e)
            .expect("the snapshot block must close before the rest of the function");
        let snapshot = &phase[lock_at..snapshot_end];

        // Self-check: if the slice ever widens to the whole function this guard
        // would silently stop discriminating.
        assert!(
            snapshot.len() < phase.len() / 2,
            "the snapshot slice widened to {} of {} bytes — rescope this guard",
            snapshot.len(),
            phase.len()
        );

        let probe = format!("installed{}()", "_version");
        assert!(
            !snapshot.contains(&probe),
            "the registry guard is still alive while a 20 s PowerShell probe runs:\n{snapshot}"
        );
        // ...and the probe must still happen, just outside the lock.
        assert!(
            phase.contains(&probe) || src[fn_end..].contains(&probe),
            "installed_version() vanished from get_integrations entirely"
        );
    }

    /// B13 D2 / D-11: the outer bound on an install must be the install bound.
    /// The constant comparison alone said nothing about the call site actually
    /// using it, so this pins the call site.
    #[test]
    fn the_install_arm_is_bounded_by_the_install_timeout_not_the_toggle_timeout() {
        let src = include_str!("integration.rs");
        let fn_at = src
            .find(&format!("pub async fn toggle{}", "_integration("))
            .expect("toggle_integration must exist");
        let fn_end = src[fn_at..]
            .find(&format!("pub async fn force{}", "_reinstall_integration("))
            .map(|e| fn_at + e)
            .unwrap_or(src.len());
        let body = &src[fn_at..fn_end];

        let install_call = format!("integration.{}()", "install");
        let at = body
            .find(&install_call)
            .expect("toggle_integration must still auto-install");
        // The timeout wrapping that call sits on the same line, before it.
        let line_start = body[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line = &body[line_start..at];
        assert!(
            line.contains("INSTALL_STEP_TIMEOUT"),
            "the install arm is not bounded by the install timeout: {line}"
        );
    }
}
