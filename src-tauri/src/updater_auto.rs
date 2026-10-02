use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tauri_plugin_updater::UpdaterExt;
use tracing::{info, warn};

use crate::config::store::ConfigStore;
use crate::security_setup;
use crate::supervisor::platform::BoundedOutput;
use crate::supervisor::{ProcessInfo, Supervisor};

/// Partner binaries that can outlive their supervisor entry — a crash, or a
/// process left by a previous app run — and still hold the install tree open
/// after every tracked process has been stopped.
const ORPHAN_IMAGES: [&str; 1] = ["frynode.exe"];

/// BUG 4c: purely supervisor-managed binaries to sweep for leftover
/// UNTRACKED copies at every app LAUNCH (not just before an update install).
/// FEM is the only thing that ever starts these, so a copy still running
/// that this fresh process didn't spawn can only be a leftover from a
/// crashed or killed previous session — left alone, it races the new copy
/// startup recovery is about to start. SpaceAcres (`space-acres.exe`) and
/// Olostep (`OlostepBrowser.exe`) are deliberately EXCLUDED here: both are
/// spawned untracked by design and a leftover copy should be ADOPTED, not
/// killed (see BUG 3 / BUG 10) — killing an active farmer mid-plot, or a
/// browser window the user has open, on every FEM launch would be actively
/// harmful, not a fix.
pub const STARTUP_ORPHAN_IMAGES: [&str; 2] = ["sdk_client.exe", "titan-edge.exe"];

/// B4: the path-scoping of both orphan sweeps and of the installer hook, kept
/// in its own file so the source-scan machinery stays out of updater_auto.rs's
/// behavioural tests.
#[cfg(test)]
#[path = "updater_auto_scope_tests.rs"]
mod updater_auto_scope_tests;

/// B4: the only directories FEM is entitled to stop a process inside — its own
/// install tree and the partner storage root.
///
/// Both sweeps used to pass `taskkill /IM <image>`, which matches by image name
/// across the WHOLE session with no path filter. A user running their own
/// Titan or MystNodes install outside FEM had it force-killed on every FEM
/// launch (`kill_startup_orphans` runs from main.rs at every start) and before
/// every update. The Done-when is explicit: FEM stops only processes whose
/// image path is under its own directories.
fn fem_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
        }
    }
    roots.push(crate::integrations::download::partners_base_dir());
    roots
}

/// Pure: is `exe_path` inside one of `roots`? Case-insensitive and
/// separator-normalised, because Win32_Process reports whatever case and
/// separators the process was started with.
pub(crate) fn process_is_under(exe_path: &str, roots: &[PathBuf]) -> bool {
    let candidate = exe_path.trim();
    if candidate.is_empty() {
        return false;
    }
    let candidate = candidate.replace('/', "\\").to_lowercase();
    roots.iter().any(|root| {
        let mut root = root.to_string_lossy().replace('/', "\\").to_lowercase();
        if root.is_empty() {
            return false;
        }
        if !root.ends_with('\\') {
            root.push('\\');
        }
        candidate.starts_with(&root)
    })
}

/// Pure: the PowerShell that lists `<pid>|<ExecutablePath>` for the given
/// images. Enumerating once and deciding in Rust keeps the decision testable.
pub(crate) fn process_listing_script(images: &[&str]) -> String {
    let filter = images
        .iter()
        .map(|i| format!("Name='{i}'"))
        .collect::<Vec<_>>()
        .join(" or ");
    format!(
        "Get-CimInstance Win32_Process -Filter \"{filter}\" | \
         ForEach-Object {{ \"$($_.ProcessId)|$($_.ExecutablePath)\" }}"
    )
}

/// Pure: which of the listed processes are ours to stop.
pub(crate) fn select_orphan_pids(listing: &str, roots: &[PathBuf]) -> Vec<u32> {
    listing
        .lines()
        .filter_map(|line| {
            let (pid, path) = line.trim().split_once('|')?;
            let pid = pid.trim().parse::<u32>().ok()?;
            process_is_under(path, roots).then_some(pid)
        })
        .collect()
}

/// Stop every process of `images` whose image path is under `roots`, and
/// nothing else. Best-effort throughout: a sweep that cannot run must never
/// stop FEM starting or updating.
fn kill_orphans_under(images: &[&str], roots: &[PathBuf], when: &str) {
    if roots.is_empty() {
        warn!(when, "Could not resolve FEM's own directories — skipping the orphan sweep rather than killing by image name");
        return;
    }
    let listing = match crate::supervisor::platform::command("powershell")
        .args(["-NoProfile", "-Command", &process_listing_script(images)])
        .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
    {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(e) => {
            warn!(when, error = %e, "Orphan cleanup could not enumerate processes — continuing");
            return;
        }
    };
    let pids = select_orphan_pids(&listing, roots);
    if pids.is_empty() {
        info!(when, "No orphaned partner process of FEM's own was running");
        return;
    }
    for pid in pids {
        let pid_arg = pid.to_string();
        match crate::supervisor::platform::command("taskkill")
            .args(["/PID", &pid_arg, "/T", "/F"])
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
        {
            Ok(o) if o.status.success() => info!(when, pid, "Killed orphaned partner process"),
            Ok(o) => info!(
                when,
                pid,
                code = o.status.code(),
                "Orphaned partner process was already gone"
            ),
            Err(e) => warn!(when, pid, error = %e, "Orphan cleanup could not run — continuing"),
        }
    }
}

/// Sweep `STARTUP_ORPHAN_IMAGES` at boot — but only copies living under FEM's
/// own directories (B4).
pub(crate) fn kill_startup_orphans() {
    kill_orphans_under(&STARTUP_ORPHAN_IMAGES[..], &fem_roots(), "startup");
}

/// Settle time after the last partner process exits, before the updater
/// replaces the install tree. Process exit and Windows releasing the file
/// handle are not the same instant.
const PARTNER_STOP_SETTLE: Duration = Duration::from_secs(2);

/// BUG 1/2 ordering hazard: `release_install_tree` stops partner processes
/// but historically never told the health loop to stay off — so the 30s
/// health tick could see a partner it just stopped as `Unhealthy`/`Stopped`
/// and restart it mid-download, re-locking a file the update is about to
/// overwrite (or racing `download_and_install`'s own file replacement).
/// `RecoveryAction::Unhealthy` restarts unconditionally regardless of the
/// per-integration `enabled` flag, so this needs its own global gate rather
/// than reusing `enabled_fn`. Set true for the remainder of THIS process's
/// life once an update install begins — on Windows `download_and_install`
/// never returns (ShellExecuteW handoff + `std::process::exit(0)`), so there
/// is no in-process "update failed, resume restarts" path to wire back up;
/// a fresh launch after either outcome starts with the flag naturally false
/// again.
pub static UPDATE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// Whether the health loop's restart_fn closures must no-op right now.
pub fn restarts_suspended() -> bool {
    UPDATE_IN_PROGRESS.load(Ordering::SeqCst)
}

/// C1 review fix: RAII guard for `UPDATE_IN_PROGRESS`. `arm()` sets the flag
/// true; `Drop` resets it to false UNLESS `keep_suspended_forever()` was
/// called first. This is what makes the suspension correct on EVERY
/// non-success exit path — an explicit `Err` return, an early `?`
/// propagation, a panic during unwind — without requiring every call site
/// to remember an explicit reset. That was exactly C1's defect: the
/// original code set the flag with a bare `.store(true, ...)` and never
/// reset it anywhere, so a failed `download_and_install` (network blip,
/// disk full, antivirus interference — the exact failure class BUG 1/2
/// exists to defend against) permanently disabled every integration's
/// restart capability for the rest of that running session.
pub(crate) struct UpdateInProgressGuard {
    reset_on_drop: bool,
}

impl UpdateInProgressGuard {
    fn arm() -> Self {
        UPDATE_IN_PROGRESS.store(true, Ordering::SeqCst);
        Self {
            reset_on_drop: true,
        }
    }

    /// Call ONLY after `download_and_install` has genuinely succeeded — the
    /// app is about to restart into the new version, so the suspension must
    /// outlive this guard rather than resetting the instant this function
    /// returns, which would re-open a restart race during the jitter delay
    /// before the actual process restart fires.
    pub(crate) fn keep_suspended_forever(mut self) {
        self.reset_on_drop = false;
    }
}

impl Drop for UpdateInProgressGuard {
    fn drop(&mut self) {
        if self.reset_on_drop {
            UPDATE_IN_PROGRESS.store(false, Ordering::SeqCst);
        }
    }
}

/// State persisted across the update's app-restart boundary so the NEW
/// process launch can confirm the update actually took effect.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UpdateState {
    pub from: String,
    pub to: String,
    pub ts: u64,
}

fn update_state_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("update-state.json")
}

fn prev_binaries_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(".prev")
}

/// Outcome of comparing a persisted `UpdateState` to the version actually
/// running after restart. Pure — no filesystem access.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateOutcome {
    /// The new process reports the version the update targeted.
    Succeeded,
    /// The new process is running some OTHER version than what the update
    /// targeted — an MSI abort, Defender quarantine, or a silently-declined
    /// install can all land here.
    Mismatched { expected: String, actual: String },
}

/// Pure decision: did the update that produced `state` actually take effect,
/// given the version this launch is running as `current_version`?
pub fn evaluate_update_outcome(state: &UpdateState, current_version: &str) -> UpdateOutcome {
    if state.to == current_version {
        UpdateOutcome::Succeeded
    } else {
        UpdateOutcome::Mismatched {
            expected: state.to.clone(),
            actual: current_version.to_string(),
        }
    }
}

fn write_update_state(path: &Path, state: &UpdateState) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::File::create(path)?;
    f.write_all(json.as_bytes())
}

fn read_update_state(path: &Path) -> Option<UpdateState> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Copy the binaries an update is about to overwrite into a `.prev` holding
/// directory, so a failed update (MSI abort, Defender quarantine, crash
/// mid-copy) leaves a known-good fallback the operator can restore by hand.
/// Best-effort per file — a missing `frynode.exe` (never installed on this
/// device) must not fail the whole backup.
pub fn backup_pre_update_binaries(files: &[(&Path, &str)], prev_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(prev_dir)?;
    for (src, dest_name) in files {
        if !src.exists() {
            continue;
        }
        std::fs::copy(src, prev_dir.join(dest_name))?;
    }
    Ok(())
}

/// Called once, early in `setup()`, before this launch does anything else
/// that assumes a clean state. Reads `update-state.json` if a previous
/// launch of this app was in the middle of an update:
/// - Version matches `to` → the update succeeded. Clean up `.prev` and the
///   state file so this check is a no-op on every subsequent normal launch.
/// - Version does not match → surface it. `.prev` and the state file are
///   left in place for manual recovery / diagnosis; nothing here auto-rolls
///   back, since an unattended rollback of a partially-applied Windows
///   install carries its own risk.
pub fn check_update_outcome_on_launch(
    app_data_dir: &Path,
    current_version: &str,
) -> Option<UpdateOutcome> {
    let state_path = update_state_path(app_data_dir);
    let state = read_update_state(&state_path)?;
    let outcome = evaluate_update_outcome(&state, current_version);
    match &outcome {
        UpdateOutcome::Succeeded => {
            info!(
                from = %state.from,
                to = %state.to,
                "Update completed successfully — clearing update state"
            );
            let _ = std::fs::remove_file(&state_path);
            let _ = std::fs::remove_dir_all(prev_binaries_dir(app_data_dir));
        }
        UpdateOutcome::Mismatched { expected, actual } => {
            warn!(
                expected = %expected,
                actual = %actual,
                "Update did not take effect — this launch is running a different version than the update targeted"
            );
            crate::events::emit(
                "update-failed",
                serde_json::json!({
                    "expected": expected,
                    "actual": actual,
                    "reason": "The app restarted after an update, but is running a different version than expected. The previous version's files are preserved for recovery.",
                }),
            );
        }
    }
    Some(outcome)
}

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
pub(crate) fn stop_partner_processes(supervisor: &Arc<Mutex<Supervisor>>) -> Vec<String> {
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
pub(crate) fn kill_orphan_partners() {
    kill_orphans_under(&ORPHAN_IMAGES[..], &fem_roots(), "pre-update");
}

/// Release the install tree before ANY updater install — shared by the
/// background auto-updater and the manual Updates-page path (B7: partner
/// binaries are launched FROM the install dir the updater replaces, so a
/// running or orphaned one keeps the NSIS copy step failing on a locked file).
///
/// ManagedProcess::stop blocks up to 10s per process, so both halves run
/// under block_in_place rather than directly on an async worker.
pub(crate) async fn release_install_tree(supervisor: &Arc<Mutex<Supervisor>>) -> Vec<String> {
    // C1 review fix: flag lifecycle is now owned by `UpdateInProgressGuard`,
    // constructed by `prepare_for_update_install` (this fn's only caller)
    // BEFORE calling this — see that function and the guard's own docs.
    // This function itself no longer sets/clears the flag directly.
    let stopped = tokio::task::block_in_place(|| stop_partner_processes(supervisor));
    tokio::task::block_in_place(kill_orphan_partners);
    if !stopped.is_empty() {
        info!(
            stopped = stopped.join(",").as_str(),
            "Stopped partner processes before update install"
        );
        tokio::time::sleep(PARTNER_STOP_SETTLE).await;
    }
    stopped
}

/// An HKLM `Uninstall` registry entry for a Windows Installer (MSI) package
/// matching Fry Edge Miner. Its presence means a THIRD install mechanism
/// (msiexec) owns some or all of this install, and NSIS silently overwriting
/// those files can leave the MSI's own uninstall entry pointing at a tree it
/// no longer fully owns — or, worse, the next `msiexec /x` a support runbook
/// runs can delete files the NSIS-updated app now depends on.
#[derive(Debug, Clone, PartialEq)]
pub struct MsiUninstallEntry {
    pub display_name: String,
    pub publisher: String,
    pub uninstall_string: String,
}

/// Parse the JSON `ConvertTo-Json` emits for the Uninstall registry query.
/// PowerShell emits a bare object for exactly one match, an array for
/// zero-or-many, and nothing/empty for zero. Pure — no registry/process I/O.
pub fn parse_msi_uninstall_json(json: &str) -> Option<MsiUninstallEntry> {
    let trimmed = json.trim();
    if trimmed.is_empty() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    let obj = match &value {
        serde_json::Value::Array(items) => items.first()?,
        serde_json::Value::Object(_) => &value,
        _ => return None,
    };
    let display_name = obj.get("DisplayName")?.as_str()?.to_string();
    let publisher = obj
        .get("Publisher")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let uninstall_string = obj
        .get("UninstallString")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // Only an msiexec-driven entry is the concern here — an NSIS uninstall
    // entry for this same app is normal and must not trip this check.
    if !uninstall_string.to_lowercase().contains("msiexec") {
        return None;
    }
    Some(MsiUninstallEntry {
        display_name,
        publisher,
        uninstall_string,
    })
}

/// Query HKLM `Uninstall` for an msiexec-owned Fry Edge Miner entry.
/// Best-effort: any PowerShell/registry failure is treated as "none found"
/// rather than blocking the update — a query error is far more likely than a
/// genuine MSI install existing alongside the NSIS one.
/// BUG 10/RC1: the registry sweep spans two Uninstall hives and every
/// installed program. On a well-used machine that genuinely exceeds the 20 s
/// generic probe budget, and a timeout used to read as "no MSI installed".
pub(crate) const MSI_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Three-state MSI ownership probe. "Could not tell" MUST be distinguishable
/// from "there is none" — conflating them is what let the update proceed into
/// the uninstall path.
#[derive(Debug)]
pub(crate) enum MsiProbe {
    Found(MsiUninstallEntry),
    NotFound,
    Inconclusive(String),
}

pub(crate) enum MsiGate {
    Proceed,
    Blocked(String),
}

/// PURE: classify the probe's raw result. Only a CLEAN exit that parsed to no
/// entry is `NotFound`; everything else is `Inconclusive`.
pub(crate) fn msi_probe_outcome(exit_code: Option<i32>, stdout: &str) -> MsiProbe {
    match exit_code {
        Some(0) => match parse_msi_uninstall_json(stdout) {
            Some(entry) => MsiProbe::Found(entry),
            None => MsiProbe::NotFound,
        },
        Some(code) => MsiProbe::Inconclusive(format!("the MSI check exited {code}")),
        None => MsiProbe::Inconclusive(
            "the MSI check did not finish (timed out or could not be started)".to_string(),
        ),
    }
}

/// PURE: fail CLOSED. An update may only proceed when we can PROVE this install
/// is not MSI-owned.
pub(crate) fn msi_gate(probe: MsiProbe) -> MsiGate {
    match probe {
        MsiProbe::NotFound => MsiGate::Proceed,
        MsiProbe::Found(entry) => MsiGate::Blocked(format!(
            "This copy of Fry Edge Miner was installed from the .msi package, which the              updater cannot safely replace. Uninstall it first, then install the current              version — your miner key and wallet are preserved. Uninstall command: {}",
            msi_remediation_command(&entry.uninstall_string)
        )),
        MsiProbe::Inconclusive(why) => MsiGate::Blocked(format!(
            "Fry Edge Miner paused this update because it could not confirm how this copy              was installed ({why}). It will try again automatically."
        )),
    }
}

/// PURE: the uninstall command with the flags the NSIS template omits — `/qn`
/// and `REBOOT=ReallySuppress` are exactly what stop the machine rebooting
/// mid-uninstall, which is how the reported incident ended.
pub(crate) fn msi_remediation_command(uninstall_string: &str) -> String {
    let base = uninstall_string.trim();
    if base.to_lowercase().contains("/qn") {
        base.to_string()
    } else {
        format!("{base} /qn /norestart REBOOT=ReallySuppress")
    }
}

/// Probe MSI ownership, distinguishing "there is none" from "could not tell".
///
/// Replaces the old `find_msi_uninstall_entry`, whose `Option` return could not
/// express the difference — and so reported every PowerShell failure and every
/// timeout as "no MSI installed".
pub(crate) fn probe_msi_ownership() -> MsiProbe {
    // Raw string: the registry paths are full of backslashes and this is the
    // one place an escaping slip would silently query nothing and then be
    // reported as "no MSI installed".
    let script = r"Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -like 'Fry Edge Miner*' } | Select-Object DisplayName,Publisher,UninstallString | ConvertTo-Json -Compress";
    match crate::supervisor::platform::command("powershell")
        .args(["-NoProfile", "-Command", script])
        .output_bounded(MSI_PROBE_TIMEOUT)
    {
        Ok(out) => msi_probe_outcome(out.status.code(), &String::from_utf8_lossy(&out.stdout)),
        Err(e) => MsiProbe::Inconclusive(format!("the MSI check could not run: {e}")),
    }
}

/// Result of the pre-install preparation step. The caller uses this to decide
/// whether to proceed to `download_and_install` at all.
pub enum PrepareOutcome {
    /// Safe to proceed — partners stopped, binaries backed up, state written.
    /// Carries the `UPDATE_IN_PROGRESS` guard: the caller MUST hold this
    /// across its own `download_and_install` call and call
    /// `keep_suspended_forever()` only in the `Ok` arm — letting it drop in
    /// any other arm (including via `?`/early return) correctly resets the
    /// suspension (C1 review fix).
    Ready(UpdateInProgressGuard),
    /// An msiexec-owned install of this app is registered; auto-updating
    /// over it silently is unsafe (see `MsiUninstallEntry` docs). The caller
    /// must skip `download_and_install` entirely. The flag was never set
    /// true for this outcome (the MSI check runs before `release_install_tree`),
    /// so there is nothing to reset here.
    MsiBlocked(MsiUninstallEntry),
}

/// Single choke point both the background auto-updater and the manual
/// Updates-page path call before `download_and_install`. Order matters:
/// MSI check (cheap, and must abort BEFORE anything else if it trips) →
/// release the install tree (stops partners, suspends restarts) → back up
/// the binaries about to be overwritten → persist the from/to state so the
/// next launch can confirm the update actually landed.
pub(crate) async fn prepare_for_update_install(
    supervisor: &Arc<Mutex<Supervisor>>,
    config: &Arc<ConfigStore>,
    app_data_dir: &Path,
    exe_path: &Path,
    frynode_path: Option<&Path>,
    from_version: &str,
    to_version: &str,
) -> PrepareOutcome {
    // BUG 10/RC1: FAIL CLOSED. The old guard returned None on any PowerShell
    // error or timeout, which read as "no MSI installed" and let the update
    // walk straight into the NSIS WiX uninstall path this exists to prevent.
    let probe = tokio::task::block_in_place(probe_msi_ownership);
    let probe_reason = match &probe {
        MsiProbe::Found(e) => Some((
            "msi-present",
            e.uninstall_string.clone(),
            e.display_name.clone(),
        )),
        MsiProbe::Inconclusive(why) => Some(("probe-inconclusive", String::new(), why.clone())),
        MsiProbe::NotFound => None,
    };
    if let MsiGate::Blocked(message) = msi_gate(probe) {
        let (reason, uninstall_string, detail) =
            probe_reason.unwrap_or(("probe-inconclusive", String::new(), String::new()));
        warn!(reason = reason, detail = %detail, "Update blocked before install");
        crate::events::emit(
            "update-blocked-msi",
            serde_json::json!({
                "reason": reason,
                "detail": detail,
                "uninstallString": uninstall_string,
                "message": message,
            }),
        );
        return PrepareOutcome::MsiBlocked(MsiUninstallEntry {
            display_name: "Fry Edge Miner".to_string(),
            publisher: String::new(),
            uninstall_string,
        });
    }

    // C1 review fix: arm the guard BEFORE touching any partner process —
    // everything from here to the caller's `download_and_install` call is
    // covered, and any early return (including the backup/state-write
    // warn-and-continue paths below, which never early-return, but also any
    // FUTURE early return added here) resets the flag automatically via
    // Drop unless the caller later calls `keep_suspended_forever()`.
    let guard = UpdateInProgressGuard::arm();

    release_install_tree(supervisor).await;

    let prev_dir = prev_binaries_dir(app_data_dir);
    let mut files: Vec<(&Path, &str)> = vec![(exe_path, "fry-edge-miner.exe")];
    if let Some(frynode) = frynode_path {
        files.push((frynode, "frynode.exe"));
    }
    if let Err(e) = backup_pre_update_binaries(&files, &prev_dir) {
        warn!(error = %e, "Could not back up pre-update binaries — continuing anyway");
    }

    let state = UpdateState {
        from: from_version.to_string(),
        to: to_version.to_string(),
        ts: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };
    if let Err(e) = write_update_state(&update_state_path(app_data_dir), &state) {
        warn!(error = %e, "Could not persist update-state.json — continuing anyway");
    }

    // BUG 1/2 (part d): re-assert the Defender exclusion + firewall
    // hardening for the version we're ABOUT to install, before Defender can
    // scan the freshly-downloaded binary. Best-effort — a decline/failure
    // here must never block the update itself.
    let recorded = config.get().hardening_applied_version;
    if security_setup::should_run_hardening(recorded.as_deref(), to_version) {
        if let Some(install_dir) = exe_path.parent() {
            let exe_names = ["fry-edge-miner.exe", "frynode.exe"];
            let frynode = frynode_path
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| install_dir.join("resources").join("frynode.exe"));
            // B3: an update is not a user action either. `to_version` is the
            // attempt key, so the new version is a new request and still gets
            // its one attempt once the user asks for it.
            match tokio::task::block_in_place(|| {
                security_setup::run_hardening_elevated(
                    install_dir,
                    &exe_names,
                    &frynode,
                    to_version,
                    crate::elevation_gate::ElevationTrigger::Automatic,
                )
            }) {
                Ok(()) => {
                    if let Err(e) = config.update(|c| {
                        c.hardening_applied_version = Some(to_version.to_string());
                    }) {
                        warn!(error = %e, "Could not persist hardening_applied_version before update");
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Pre-update hardening declined or failed — continuing update anyway");
                    // B3 defect 4: same reason as the boot path — a warn! alone
                    // never reaches the user.
                    crate::events::emit(
                        "elevation-required",
                        serde_json::json!({
                            "purpose": "hardening",
                            "reason": e.to_string(),
                        }),
                    );
                }
            }
        }
    }

    PrepareOutcome::Ready(guard)
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

    // BUG 1/2: MSI-ownership check → release install tree (suspends
    // restarts) → back up the binaries about to be overwritten → persist
    // from/to state for the next launch to confirm. Replaces the bare
    // `release_install_tree` call (B7) with the full pre-install sequence.
    use tauri::Manager;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir());
    let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("fry-edge-miner.exe"));
    let frynode_path = exe_path
        .parent()
        .map(|d| d.join("resources").join("frynode.exe"));
    let guard = match prepare_for_update_install(
        supervisor,
        config,
        &app_data_dir,
        &exe_path,
        frynode_path.as_deref(),
        current,
        &update.version,
    )
    .await
    {
        PrepareOutcome::MsiBlocked(_) => {
            return Ok(Some(
                "Update skipped — an MSI install is registered; uninstall it first".to_string(),
            ));
        }
        PrepareOutcome::Ready(guard) => guard,
    };

    // Download and install. `guard` stays in scope across this call — a
    // failure below drops it at the `return`/end-of-match, resetting
    // UPDATE_IN_PROGRESS automatically (C1 review fix); only the Ok arm
    // explicitly keeps the suspension alive past this function returning.
    match update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
    {
        Ok(()) => {
            info!(version = %update.version, "Update installed successfully");
            guard.keep_suspended_forever();

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
            // C1 review fix: `guard` drops here (end of this arm/function),
            // resetting UPDATE_IN_PROGRESS — a failed download must not
            // permanently disable every integration's restart capability.
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
    let seed_str = cfg.install_id.as_ref().cloned().unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos().to_string())
            .unwrap_or_else(|_| "fallback".to_string())
    });

    // Simple hash: sum the bytes mod MAX_JITTER_SECS
    let hash = seed_str.as_bytes().iter().map(|b| *b as u64).sum::<u64>() % MAX_JITTER_SECS;

    hash
}

/// These two tests used to define their own private copies of the jitter maths
/// and of an `auto_update && current != latest` predicate, and called nothing
/// from the parent module — which is why the `use super::*` above them was
/// flagged as unused. They could not fail for any change to shipped code.
///
/// The two halves are not symmetric, and that decides how each is rewritten:
///
/// * Jitter has a real function — `compute_jitter_secs` — so these call it.
/// * The auto-install decision has NO real counterpart anywhere in the crate.
///   It is two `if`s in two different `async fn`s with an awaited network call
///   between them, so it cannot be called from a test at all. It is pinned by
///   source assertion instead, which is what the rest of this crate does for
///   loops that never return.
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A real `ConfigStore` on a real (temporary) directory. The `TempDir` is
    /// returned rather than dropped: dropping it deletes the directory out from
    /// under the store.
    fn store_with(install_id: Option<&str>) -> (tempfile::TempDir, Arc<ConfigStore>) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(ConfigStore::new(dir.path().to_path_buf(), None));
        store
            .update(|c| c.install_id = install_id.map(|s| s.to_string()))
            .expect("seed install_id");
        (dir, store)
    }

    /// Fleet-wide simultaneous restarts are the thing jitter exists to prevent,
    /// and the window is what bounds the damage.
    #[test]
    fn jitter_stays_inside_the_ten_minute_window() {
        for id in ["install-abc123", "install-xyz789", ""] {
            let (_dir, store) = store_with(Some(id));
            let jitter = compute_jitter_secs(&store);
            assert!(jitter < 600, "jitter {jitter} out of bounds for id {id:?}");
        }
    }

    /// A device must land in the same slot every cycle. Jitter that moved on
    /// each call would spread one device across the window instead of spreading
    /// the fleet across it.
    #[test]
    fn jitter_is_deterministic_for_one_device() {
        let (_dir, store) = store_with(Some("install-abc123"));
        let first = compute_jitter_secs(&store);
        for _ in 0..5 {
            assert_eq!(
                compute_jitter_secs(&store),
                first,
                "the same install_id must always produce the same jitter"
            );
        }
    }

    /// The seed must actually be the install_id. If the function ignored it,
    /// every device in the fleet would compute the SAME offset and jitter would
    /// be decorative — the exact failure it exists to prevent.
    #[test]
    fn the_install_id_is_what_seeds_the_jitter() {
        let (_a, store_a) = store_with(Some("aaaa"));
        let (_b, store_b) = store_with(Some("aaab"));
        assert_ne!(
            compute_jitter_secs(&store_a),
            compute_jitter_secs(&store_b),
            "two different install_ids must not collide on the same jitter"
        );
    }

    /// The branch the old private copy did not have at all: with no install_id
    /// the real function falls back to `SystemTime` nanos, which is a moving
    /// value and must still be bounded.
    #[test]
    fn a_device_with_no_install_id_still_gets_bounded_jitter() {
        let (_dir, store) = store_with(None);
        assert!(store.get().install_id.is_none(), "fixture must have no id");
        for _ in 0..20 {
            let jitter = compute_jitter_secs(&store);
            assert!(jitter < 600, "fallback jitter {jitter} out of bounds");
        }
    }

    fn code_only(src: &str) -> String {
        src.lines()
            .map(|l| l.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Source between two markers, comments stripped. Scoped to a region rather
    /// than the whole file so a match somewhere else cannot satisfy it.
    fn region(from: &str, to: &str) -> String {
        let code = code_only(include_str!("updater_auto.rs"));
        let start = code.find(from).unwrap_or_else(|| panic!("missing {from}"));
        let end = code[start..]
            .find(to)
            .map(|i| start + i)
            .unwrap_or(code.len());
        code[start..end].to_string()
    }

    /// The auto_update config flag must gate the cycle. Deleting this check
    /// would make FEM install updates on devices whose owner turned auto-update
    /// off — which is why it is worth pinning even though it cannot be called.
    #[test]
    fn the_update_cycle_is_gated_on_the_auto_update_flag() {
        let spawn = region(
            "pub async fn spawn_auto_updater",
            "async fn check_and_install",
        );
        // Assembled at runtime: this file is what the assertion scans, so a
        // contiguous literal here would match itself.
        let gate = format!("!cfg.auto{}update", '_');
        let call = format!("check_and_install{}update(", '_');
        assert!(
            spawn.contains(&gate),
            "the update cycle must skip when auto_update is off"
        );
        let gate_at = spawn.find(&gate).expect("gate present");
        let call_at = spawn.find(&call).expect("the cycle must run the check");
        assert!(
            gate_at < call_at,
            "the flag must be checked BEFORE the update check runs, or a device \
             with auto-update disabled still reaches the installer"
        );
    }

    /// The defensive equality check that stops a re-install of the version
    /// already running.
    #[test]
    fn an_update_matching_the_running_version_is_not_installed() {
        let check = region(
            "async fn check_and_install_update",
            "fn compute_jitter_secs",
        );
        let needle = format!("update.version == cur{}", "rent");
        assert!(
            check.contains(&needle),
            "the installer must skip an update whose version equals the running one"
        );
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

    /// BUG 4c: the startup sweep covers Myst and Titan's purely
    /// supervisor-managed binaries...
    #[test]
    fn the_startup_sweep_covers_purely_supervisor_managed_binaries() {
        assert!(STARTUP_ORPHAN_IMAGES.contains(&"sdk_client.exe"));
        assert!(STARTUP_ORPHAN_IMAGES.contains(&"titan-edge.exe"));
    }

    /// ...and deliberately never includes the two binaries FEM spawns
    /// untracked by design, where a leftover copy must be adopted rather
    /// than killed out from under an active farmer or an open browser
    /// window (BUG 3 / BUG 10).
    #[test]
    fn the_startup_sweep_never_kills_untracked_by_design_processes() {
        assert!(!STARTUP_ORPHAN_IMAGES.contains(&"space-acres.exe"));
        assert!(!STARTUP_ORPHAN_IMAGES.contains(&"OlostepBrowser.exe"));
    }

    #[test]
    fn the_settle_wait_is_bounded_and_short() {
        // Long enough for Windows to release the handle, short enough that it
        // cannot stall the 6-hour check loop in any meaningful way.
        assert!(PARTNER_STOP_SETTLE >= Duration::from_secs(1));
        assert!(PARTNER_STOP_SETTLE <= Duration::from_secs(10));
    }
}

/// C1 review fix: `UpdateInProgressGuard` regression tests. These DO touch
/// the real process-global `UPDATE_IN_PROGRESS` — safe now specifically
/// because the guard GUARANTEES a reset via `Drop` unless explicitly told
/// not to, so every test here restores the flag to `false` before it ends.
/// Grep confirms no other test in this codebase reads/writes this static, so
/// the only ordering risk is these tests racing EACH OTHER under default
/// (parallel) `cargo test` — guarded by `GUARD_TEST_LOCK` for that reason.
#[cfg(test)]
mod update_in_progress_guard_tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    static GUARD_TEST_LOCK: StdMutex<()> = StdMutex::new(());

    /// The exact regression C1 reported: a failed `download_and_install`
    /// (network blip, disk full, antivirus interference) must not
    /// permanently disable the health loop's restart capability for the
    /// rest of the running session.
    #[test]
    fn a_dropped_guard_without_keep_suspended_forever_resets_the_flag() {
        let _lock = GUARD_TEST_LOCK.lock().unwrap();
        assert!(!restarts_suspended(), "flag must start false");

        {
            let guard = UpdateInProgressGuard::arm();
            assert!(restarts_suspended(), "arm() must suspend restarts");
            // Simulates the Err arm of `download_and_install` — the guard
            // simply falls out of scope without `keep_suspended_forever()`.
            drop(guard);
        }

        assert!(
            !restarts_suspended(),
            "a dropped guard (failed download) must reset the suspension — this is the exact C1 regression"
        );
    }

    #[test]
    fn a_successful_update_keeps_the_suspension_alive_past_the_guards_scope() {
        let _lock = GUARD_TEST_LOCK.lock().unwrap();
        assert!(!restarts_suspended(), "flag must start false");

        let guard = UpdateInProgressGuard::arm();
        guard.keep_suspended_forever();

        assert!(
            restarts_suspended(),
            "a successful update must keep restarts suspended past the guard's own scope \
             (the app is about to restart; resetting here would re-open the mid-restart race)"
        );

        // Restore steady state for any other test sharing this process.
        UPDATE_IN_PROGRESS.store(false, Ordering::SeqCst);
    }

    #[test]
    fn an_early_return_via_question_mark_still_drops_and_resets() {
        // Mirrors the exact shape both call sites use: bind the guard from
        // a match, then `?`/early-return out of a fallible operation before
        // ever calling `keep_suspended_forever()`.
        fn simulate_failed_install() -> Result<(), String> {
            let _guard = UpdateInProgressGuard::arm();
            Err::<(), String>("Download/install failed".to_string())?;
            unreachable!()
        }

        let _lock = GUARD_TEST_LOCK.lock().unwrap();
        assert!(!restarts_suspended());
        let result = simulate_failed_install();
        assert!(result.is_err());
        assert!(
            !restarts_suspended(),
            "an early return via ? must still drop the guard and reset the flag"
        );
    }
}

/// BUG 1/2: updater safety-net tests. None of these touch `UPDATE_IN_PROGRESS`
/// (a process-global `AtomicBool`) — `release_install_tree`/
/// `prepare_for_update_install` are deliberately NOT exercised here, since
/// flipping that flag would leak into every other test sharing this test
/// binary's process.
#[cfg(test)]
mod update_safety_tests {
    use super::*;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fem-updater-test-{}-{}-{}",
            name,
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // --- evaluate_update_outcome -------------------------------------------

    #[test]
    fn a_matching_version_after_restart_is_success() {
        let state = UpdateState {
            from: "0.4.27".into(),
            to: "0.4.28".into(),
            ts: 0,
        };
        assert_eq!(
            evaluate_update_outcome(&state, "0.4.28"),
            UpdateOutcome::Succeeded
        );
    }

    #[test]
    fn a_different_version_after_restart_is_a_mismatch_with_both_versions_named() {
        let state = UpdateState {
            from: "0.4.27".into(),
            to: "0.4.28".into(),
            ts: 0,
        };
        let outcome = evaluate_update_outcome(&state, "0.4.27");
        assert_eq!(
            outcome,
            UpdateOutcome::Mismatched {
                expected: "0.4.28".into(),
                actual: "0.4.27".into()
            }
        );
    }

    // --- update-state.json round trip (real filesystem, tempdir) -----------

    #[test]
    fn update_state_round_trips_through_disk() {
        let dir = tmp_dir("state-roundtrip");
        let path = update_state_path(&dir);
        let state = UpdateState {
            from: "0.4.27".into(),
            to: "0.4.28".into(),
            ts: 12345,
        };
        write_update_state(&path, &state).expect("write must succeed");
        let read_back = read_update_state(&path).expect("state file must be readable");
        assert_eq!(read_back, state);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_state_file_reads_as_none_not_an_error() {
        let dir = tmp_dir("state-missing");
        let path = update_state_path(&dir);
        assert!(read_update_state(&path).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- backup_pre_update_binaries ------------------------------------------

    #[test]
    fn present_binaries_are_copied_into_the_prev_dir() {
        let dir = tmp_dir("backup-present");
        let exe_src = dir.join("fry-edge-miner.exe");
        std::fs::write(&exe_src, b"fake exe bytes").unwrap();
        let prev = dir.join(".prev");

        backup_pre_update_binaries(&[(&exe_src, "fry-edge-miner.exe")], &prev)
            .expect("backup must succeed");

        let copied = prev.join("fry-edge-miner.exe");
        assert!(copied.exists());
        assert_eq!(std::fs::read(&copied).unwrap(), b"fake exe bytes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_optional_binary_like_frynode_never_seen_on_this_device_is_skipped_not_an_error() {
        let dir = tmp_dir("backup-missing-optional");
        let missing = dir.join("frynode.exe"); // never created
        let prev = dir.join(".prev");

        let result = backup_pre_update_binaries(&[(&missing, "frynode.exe")], &prev);
        assert!(
            result.is_ok(),
            "a never-installed optional binary must not fail the backup"
        );
        assert!(!prev.join("frynode.exe").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- check_update_outcome_on_launch (fs-only; does not touch the atomic) -

    #[test]
    fn a_successful_outcome_cleans_up_state_and_prev_dir() {
        let dir = tmp_dir("launch-success");
        let state = UpdateState {
            from: "0.4.27".into(),
            to: "0.4.28".into(),
            ts: 0,
        };
        write_update_state(&update_state_path(&dir), &state).unwrap();
        std::fs::create_dir_all(prev_binaries_dir(&dir)).unwrap();
        std::fs::write(prev_binaries_dir(&dir).join("fry-edge-miner.exe"), b"old").unwrap();

        let outcome = check_update_outcome_on_launch(&dir, "0.4.28");
        assert_eq!(outcome, Some(UpdateOutcome::Succeeded));
        assert!(
            !update_state_path(&dir).exists(),
            "state file must be cleared on success"
        );
        assert!(
            !prev_binaries_dir(&dir).exists(),
            ".prev must be cleared on success"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_mismatched_outcome_leaves_prev_dir_and_state_for_manual_recovery() {
        let dir = tmp_dir("launch-mismatch");
        let state = UpdateState {
            from: "0.4.27".into(),
            to: "0.4.28".into(),
            ts: 0,
        };
        write_update_state(&update_state_path(&dir), &state).unwrap();
        std::fs::create_dir_all(prev_binaries_dir(&dir)).unwrap();
        std::fs::write(prev_binaries_dir(&dir).join("fry-edge-miner.exe"), b"old").unwrap();

        // Running 0.4.27 after an update that targeted 0.4.28 — e.g. an MSI
        // abort or Defender quarantine silently kept the old binary in place.
        let outcome = check_update_outcome_on_launch(&dir, "0.4.27");
        assert_eq!(
            outcome,
            Some(UpdateOutcome::Mismatched {
                expected: "0.4.28".into(),
                actual: "0.4.27".into()
            })
        );
        assert!(
            update_state_path(&dir).exists(),
            "state file must survive for diagnosis"
        );
        assert!(
            prev_binaries_dir(&dir).join("fry-edge-miner.exe").exists(),
            ".prev must survive so the operator can restore the last-known-good binary"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_state_file_present_is_a_normal_launch_not_an_error() {
        let dir = tmp_dir("launch-normal");
        assert_eq!(check_update_outcome_on_launch(&dir, "0.4.28"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- MSI uninstall entry parsing (pure) ---------------------------------

    #[test]
    fn a_single_msiexec_owned_entry_parses_as_blocked() {
        let json = r#"{"DisplayName":"Fry Edge Miner","Publisher":"frynetworks","UninstallString":"MsiExec.exe /X{GUID}"}"#;
        let entry = parse_msi_uninstall_json(json).expect("must parse a single object");
        assert_eq!(entry.display_name, "Fry Edge Miner");
        assert_eq!(entry.publisher, "frynetworks");
    }

    #[test]
    fn an_nsis_owned_entry_for_the_same_app_is_not_msi_blocked() {
        // The NSIS installer ALSO registers an Uninstall entry for "Fry Edge
        // Miner" — its UninstallString runs the NSIS uninstaller, not
        // msiexec. That must never trip the MSI-block path.
        let json = r#"{"DisplayName":"Fry Edge Miner","Publisher":"frynetworks","UninstallString":"C:\\Users\\x\\AppData\\Local\\Fry Edge Miner\\uninstall.exe"}"#;
        assert!(parse_msi_uninstall_json(json).is_none());
    }

    #[test]
    fn an_array_of_matches_uses_the_first_msiexec_owned_entry() {
        let json = r#"[{"DisplayName":"Fry Edge Miner","Publisher":"frynetworks","UninstallString":"MsiExec.exe /X{GUID}"},{"DisplayName":"Fry Edge Miner Helper","Publisher":"frynetworks","UninstallString":"MsiExec.exe /X{GUID2}"}]"#;
        let entry = parse_msi_uninstall_json(json).expect("must parse an array");
        assert_eq!(entry.display_name, "Fry Edge Miner");
    }

    #[test]
    fn empty_output_means_no_entry_found() {
        assert!(parse_msi_uninstall_json("").is_none());
        assert!(parse_msi_uninstall_json("   ").is_none());
    }

    #[test]
    fn malformed_json_is_treated_as_no_entry_rather_than_a_panic() {
        assert!(parse_msi_uninstall_json("not json").is_none());
    }
}

/// BUG 10 / RC1 (RailgunDude): "After a FEM update, FEM ended up UNINSTALLED
/// and the PC rebooted."
///
/// `bundle.targets: "all"` shipped an MSI alongside the NSIS setup.exe. In the
/// generated NSIS `PageLeaveReinstall`, the WiX/MSI check runs BEFORE the
/// `/UPDATE` short-circuit and runs the MSI `UninstallString` raw — unelevated
/// from NSIS's view, no `/qn`, no `REBOOT=ReallySuppress` — then `MessageBox` +
/// `Abort` if it returns non-zero OR if the exe merely still exists. The old
/// app is gone, the new one was never installed, and FEM itself is already
/// dead because `download_and_install` hands off via ShellExecuteW and calls
/// `exit(0)`. `msiexec` is also the only component in that chain privileged
/// enough to schedule delayed file renames and reboot the machine.
///
/// v0.4.28's guard FAILED OPEN: any PowerShell error or a PROBE_TIMEOUT expiry
/// yielded `None`, which read as "no MSI installed" and let the update proceed
/// into the exact path the guard exists to prevent. On a machine with several
/// hundred installed programs that registry sweep can genuinely exceed 20 s.
#[cfg(test)]
mod bug10_msi_gate_tests {
    use super::*;

    #[test]
    fn the_bundle_never_ships_an_msi_target() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        let targets = &conf["bundle"]["targets"];
        assert!(
            targets.is_array(),
            "bundle.targets must be an explicit list, not \"all\" (got {targets})"
        );
        let listed: Vec<&str> = targets
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t.as_str())
            .collect();
        assert!(
            !listed.iter().any(|t| t.eq_ignore_ascii_case("msi")),
            "shipping an MSI re-arms the NSIS WiX uninstall path: {listed:?}"
        );
        assert!(
            listed.contains(&"nsis"),
            "the updater feed needs the NSIS installer"
        );
    }

    #[test]
    fn an_inconclusive_probe_never_reads_as_no_msi_installed() {
        assert!(matches!(
            msi_probe_outcome(None, ""),
            MsiProbe::Inconclusive(_)
        ));
        assert!(matches!(
            msi_probe_outcome(Some(1), ""),
            MsiProbe::Inconclusive(_)
        ));
        // A clean run that genuinely found nothing is the ONLY NotFound.
        assert!(matches!(msi_probe_outcome(Some(0), ""), MsiProbe::NotFound));
    }

    #[test]
    fn an_inconclusive_probe_blocks_the_install() {
        assert!(matches!(
            msi_gate(MsiProbe::Inconclusive("timed out".into())),
            MsiGate::Blocked(_)
        ));
        assert!(matches!(msi_gate(MsiProbe::NotFound), MsiGate::Proceed));
        let entry = MsiUninstallEntry {
            display_name: "Fry Edge Miner".into(),
            publisher: "Fry Networks".into(),
            uninstall_string: "MsiExec.exe /X{GUID}".into(),
        };
        assert!(matches!(
            msi_gate(MsiProbe::Found(entry)),
            MsiGate::Blocked(_)
        ));
    }

    /// The registry sweep is far slower than a generic CLI probe, and timing
    /// out is precisely what made the v0.4.28 guard fail open in the field.
    #[test]
    fn the_msi_probe_gets_a_budget_that_fits_a_real_registry_sweep() {
        assert!(MSI_PROBE_TIMEOUT > crate::supervisor::platform::PROBE_TIMEOUT);
        assert!(MSI_PROBE_TIMEOUT <= std::time::Duration::from_secs(120));
    }

    /// A blocked user must get a command they can actually run. The NSIS
    /// template omits exactly the flags that prevent the reboot.
    #[test]
    fn the_remediation_command_suppresses_the_reboot_that_caused_the_incident() {
        let cmd = msi_remediation_command("MsiExec.exe /X{ABC-123}");
        assert!(
            cmd.contains("/X{ABC-123}"),
            "must target the real product: {cmd}"
        );
        assert!(cmd.contains("/qn"), "must be silent: {cmd}");
        assert!(
            cmd.contains("REBOOT=ReallySuppress"),
            "must not reboot the machine: {cmd}"
        );
    }
}
