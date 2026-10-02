//! FAIL-11 (row 6): `run_hardening_elevated` had exactly one caller shape —
//! the boot pass (main.rs) and the pre-update re-assert (updater_auto.rs) —
//! and both pass `ElevationTrigger::Automatic`, which the gate refuses before
//! any UAC prompt appears. There was no path back into hardening for a user
//! who actually wants it: a declined/failed attempt publishes
//! `elevation-required` (main.rs, updater_auto.rs), but nothing in the
//! frontend listened for it, and no command could re-run hardening with a
//! trigger the gate would honour.
//!
//! `retry_hardening` is that path: a `#[tauri::command]`, reachable only from
//! an explicit user gesture (a Settings action, or the "Needs administrator
//! approval — Retry" card's Retry button), that calls
//! `run_hardening_elevated` with `ElevationTrigger::UserClick`. It is the
//! ONLY hardening call site that does.

use crate::elevation_gate::ElevationTrigger;

/// BUG LOOP 2 (BLOCKING): the boot pass's Automatic refusal publishes into
/// `elevation_gate::blocked_reasons()` (via `run_elevated`'s own
/// `publish_block`) within microseconds of app setup — long before the
/// webview has loaded, React has mounted, or the frontend's live
/// `elevation-required` listener has registered. `events::emit` is
/// fire-and-forget with no replay, so that boot-time publish was simply
/// dropped and the Retry banner never appeared on a normal boot/update.
///
/// This is the pull half: called once on mount, it returns whatever the
/// gate is CURRENTLY holding for "hardening", so a block that happened
/// before any listener existed is not lost. No `AppState` needed —
/// `blocked_reasons()` is a free function.
#[tauri::command]
pub async fn get_hardening_status() -> Result<Option<String>, String> {
    Ok(crate::elevation_gate::blocked_reasons()
        .get("hardening")
        .cloned())
}

/// The ONLY hardening call site that may pass `UserClick`. Boot
/// (main.rs) and the pre-update re-assert (updater_auto.rs) are unchanged —
/// both still pass `Automatic`.
#[tauri::command]
pub async fn retry_hardening(state: tauri::State<'_, crate::AppState>) -> Result<(), String> {
    let config = state.config.clone();
    tokio::task::block_in_place(move || {
        // NB: re-arm the one allowed attempt on EVERY real gesture — the
        // same pattern `toggle_integration` uses (commands/integration.rs,
        // `clear_blocked`) for every other card. Without this, a declined or
        // timed-out UAC prompt spends the current version's one attempt
        // forever, and every later Retry click this run is a silent
        // `AlreadyAttempted` no-op — exactly the B3 "retry gesture was a
        // guaranteed no-op" shape `clear_blocked` exists to prevent.
        crate::elevation_gate::clear_blocked("hardening");

        let current = env!("CARGO_PKG_VERSION");
        let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
        let install_dir = exe_path
            .parent()
            .ok_or_else(|| "could not determine the install directory".to_string())?;
        let frynode_path = install_dir.join("resources").join("frynode.exe");
        let exe_names = ["fry-edge-miner.exe", "frynode.exe"];

        match crate::security_setup::run_hardening_elevated(
            install_dir,
            &exe_names,
            &frynode_path,
            current,
            ElevationTrigger::UserClick,
        ) {
            Ok(()) => {
                if let Err(e) = config.update(|c| {
                    c.hardening_applied_version = Some(current.to_string());
                }) {
                    tracing::warn!(error = %e, "Could not persist hardening_applied_version");
                }
                Ok(())
            }
            Err(e) => {
                let manual =
                    crate::security_setup::manual_hardening_command(install_dir, &exe_names);
                tracing::warn!(
                    error = %e,
                    manual_command = %manual,
                    "User-initiated hardening retry declined or failed"
                );
                crate::events::emit(
                    "elevation-required",
                    serde_json::json!({
                        "purpose": "hardening",
                        "reason": e.to_string(),
                        "manualCommand": manual,
                    }),
                );
                Err(e.to_string())
            }
        }
    })
}

#[cfg(test)]
#[path = "hardening_user_click_tests.rs"]
mod hardening_user_click_tests;
