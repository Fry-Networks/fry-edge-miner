use super::download::{download_file_with_options, partners_base_dir};
use super::{HealthStatus, Integration, PocGateData};
use crate::supervisor::platform::BoundedOutput;
use crate::supervisor::Supervisor;
use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

/// BUG 8 (Discord: four Scorpion63 screenshots — VCRUNTIME140.dll /
/// VCRUNTIME140_1.dll / MSVCP140.dll / MSVCP140_ATOMIC_WAIT.dll not found).
/// titan-edge.exe is a Go+cgo binary that links the VC++ 2015-2022 runtime;
/// without it Windows fails the process launch fast (STATUS_DLL_NOT_FOUND)
/// and `health_check()` had no way to say why — same defect class as BUG 6/9.
/// `msvcp140_atomic_wait.dll` is the newest member of that runtime family
/// (added ≥14.29) and ships only when a modern-enough redist is installed,
/// so its presence is a reliable single-file proxy for "the whole family is
/// there" without a registry read.
const VC_REDIST_MARKER_DLL: &str = "msvcp140_atomic_wait.dll";

const VC_REDIST_DOWNLOAD_URL: &str = "https://aka.ms/vs/17/release/vc_redist.x64.exe";

/// Pure: does `system_root`'s System32 contain the VC++ 2015-2022 marker
/// DLL? Parameterized so it is testable against a tempdir instead of the
/// real `%SystemRoot%`.
fn vc_redist_dll_present(system_root: &Path) -> bool {
    system_root
        .join("System32")
        .join(VC_REDIST_MARKER_DLL)
        .exists()
}

/// Whether the VC++ 2015-2022 x64 runtime titan-edge.exe needs is missing on
/// this machine.
pub(crate) fn vc_redist_missing() -> bool {
    let system_root = std::env::var("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(r"C:\Windows"));
    !vc_redist_dll_present(&system_root)
}

/// H2 review fix: `crate::supervisor::platform::PROBE_TIMEOUT` (20s) is the
/// generic short-lived-CLI-probe deadline (`tasklist`, `netsh show`, one-line
/// PowerShell queries) — nowhere near enough for a real Microsoft VC++
/// redistributable install (`/quiet` still commonly takes well over 20s),
/// and that budget also has to absorb however long the user takes to notice
/// and click the UAC prompt. Bounded but materially longer: 10 minutes.
pub(crate) const VC_REDIST_INSTALL_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(600);

/// Outcome of one elevated VC++ redist install attempt. A dedicated enum
/// rather than folding everything into `Result` because `StillInstalling`
/// is NOT a failure — `output_bounded`'s timeout only kills the OUTER
/// unelevated `powershell.exe` that launched `Start-Process -Verb RunAs -WindowStyle Hidden`;
/// the real elevated installer is a separate process in a different
/// security context that kill cannot reach, so it very plausibly keeps
/// running and can succeed moments later. Reporting that as a hard failure
/// (the pre-fix behavior) defeats BUG 8's auto-remediation for the "slow but
/// working" case, which is the realistic case, not an edge case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VcRedistInstallOutcome {
    Installed,
    /// The wait budget elapsed before the outer wrapper returned. The real
    /// installer may still be running; the caller should NOT treat this as
    /// a failure requiring user action — `health_check()`'s next tick
    /// re-checks `vc_redist_missing()` for real.
    StillInstalling,
    Failed(Option<i32>),
}

/// H2 review fix, pure & testable: map the raw attempt outcome to what to
/// report. `timed_out` takes priority over the exit code/success bits
/// because a timeout means we never actually observed the elevated
/// installer's own outcome — only that the OUTER wrapper didn't return
/// in time.
fn vc_redist_install_outcome(
    timed_out: bool,
    success: bool,
    exit_code: Option<i32>,
) -> VcRedistInstallOutcome {
    if timed_out {
        return VcRedistInstallOutcome::StillInstalling;
    }
    // 3010 = success, restart required — still a success for our purposes.
    if success || exit_code == Some(3010) {
        VcRedistInstallOutcome::Installed
    } else {
        VcRedistInstallOutcome::Failed(exit_code)
    }
}

/// Download and silently install the VC++ 2015-2022 x64 redistributable
/// through ONE elevated prompt (same `Start-Process -Verb RunAs -WindowStyle Hidden -Wait`
/// pattern as `security_setup::run_hardening_elevated` /
/// `firewall::ensure_program_rules`). `Err` is reserved for genuine
/// unexpected failures (download failed, task panicked); a completed-but-declined
/// install and a still-in-progress one are both `Ok` with a distinguishing
/// `VcRedistInstallOutcome` — see that type's docs for why timeout ≠ failure.
pub(crate) async fn install_vc_redist_elevated(
    trigger: crate::elevation_gate::ElevationTrigger,
) -> Result<VcRedistInstallOutcome> {
    // B3 / G4 BLOCKER: this raised `Start-Process -Verb RunAs` directly, with
    // no reference to the elevation gate at all — while the gate's own module
    // doc listed this function as one of the five sites it covered. The boot
    // recovery pass calls `install()` for every enabled-but-not-installed
    // integration, `installed_version()` is None whenever titan-edge.exe is
    // absent (the B4 wiped-partner-files population), and `vc_redist_missing()`
    // is true on a redist-free machine (the B1 population) — so a UAC dialog
    // appeared at app start, unprompted, for exactly the users who filed those
    // two bugs. Refusing an Automatic trigger is the whole point of B3.
    if trigger == crate::elevation_gate::ElevationTrigger::Automatic {
        // Asked BEFORE downloading 25 MB we are not going to be allowed to run.
        // The gate refuses every Automatic trigger and publishes the
        // needs-approval reason against this integration's card.
        let skipped = crate::elevation_gate::run_elevated(
            "titan",
            "vc-redist|needs-approval",
            trigger,
            || Ok::<(), anyhow::Error>(()),
        )
        .expect_err("the gate always refuses an Automatic trigger");
        warn!(reason = %skipped, "VC++ redist install needs administrator approval");
        anyhow::bail!("{skipped}");
    }

    let installer_path = std::env::temp_dir().join("vc_redist.x64.exe");
    download_file_with_options(VC_REDIST_DOWNLOAD_URL, &installer_path, USER_AGENT, None).await?;

    let installer_str = installer_path.to_string_lossy().to_string();
    let outer = format!(
        "$ErrorActionPreference = 'Stop'; try {{ $p = Start-Process -FilePath '{}' -ArgumentList '/install','/quiet','/norestart' -Verb RunAs -WindowStyle Hidden -Wait -PassThru; if ($null -eq $p) {{ exit 3 }}; exit $p.ExitCode }} catch {{ exit 2 }}",
        installer_str.replace('\'', "''")
    );

    // Identity-bearing, so a different redist build re-arms the one allowed
    // attempt instead of being silently suppressed.
    let attempt_key = format!("vc-redist|{}", installer_str.to_lowercase());
    let gated = crate::elevation_gate::run_elevated("titan", &attempt_key, trigger, move || {
        crate::supervisor::platform::command("powershell")
            .args(["-NoProfile", "-Command", &outer])
            .output_bounded(VC_REDIST_INSTALL_TIMEOUT)
            .map_err(anyhow::Error::new)
    });

    let result: std::io::Result<std::process::Output> = match gated {
        Ok(out) => Ok(out),
        Err(crate::elevation_gate::ElevationSkipped::Failed(reason)) => {
            // The gate ran the closure and it failed. A TimedOut here is the
            // "still installing in the background" case below, not a failure,
            // so it has to survive the round trip through the gate.
            if reason.contains("timed out") || reason.contains("TimedOut") {
                Err(std::io::Error::new(std::io::ErrorKind::TimedOut, reason))
            } else {
                warn!(reason = %reason, "VC++ redist install did not complete");
                return Ok(vc_redist_install_outcome(false, false, None));
            }
        }
        Err(skipped) => {
            warn!(reason = %skipped, "VC++ redist install skipped by the elevation gate");
            anyhow::bail!("{skipped}")
        }
    };

    let outcome = match &result {
        Ok(out) => vc_redist_install_outcome(false, out.status.success(), out.status.code()),
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            vc_redist_install_outcome(true, false, None)
        }
        Err(e) => return Err(anyhow::anyhow!("VC++ redist install could not run: {e}")),
    };

    match outcome {
        VcRedistInstallOutcome::Installed => {
            info!("VC++ 2015-2022 x64 redistributable installed");
            Ok(outcome)
        }
        VcRedistInstallOutcome::StillInstalling => {
            warn!(
                timeout_s = VC_REDIST_INSTALL_TIMEOUT.as_secs(),
                "VC++ redist installer exceeded the wait budget — it may still be installing \
                 in the background (the outer wait wrapper was stopped, not the elevated \
                 installer itself); will re-check on the next health tick"
            );
            Ok(outcome)
        }
        VcRedistInstallOutcome::Failed(code) => {
            anyhow::bail!("VC++ redist install declined or failed (exit {:?})", code)
        }
    }
}

/// Unpacking a partner release is an INSTALL step, not a probe.
///
/// This used to be `PROBE_TIMEOUT` — the repo's own 20 s deadline for
/// short-lived CLI queries, whose doc comment says as much. On a slow disk or
/// a custom storage root on a second drive, `tar` was KILLED mid-extract and
/// the `?` aborted install after extraction and before the archive cleanup,
/// leaving exactly the reported layout: the extracted subdirectory and the
/// archive still on disk, and no titan-edge.exe at the partner-dir level. The
/// repo already recorded this same anti-pattern for the VC++ redist.
const EXTRACT_TIMEOUT: std::time::Duration = crate::supervisor::platform::LONG_TIMEOUT;

// A runtime assert over two compile-time constants can never fail, so this is a
// build-time invariant instead: unpacking a partner release must never be
// bounded by the short-lived-CLI-probe deadline again.
const _: () = assert!(
    EXTRACT_TIMEOUT.as_secs() > crate::supervisor::platform::PROBE_TIMEOUT.as_secs(),
    "the extraction deadline must outlast the generic probe deadline"
);

/// Per-file sha256 pins for the v0.1.20 release.
///
/// Measured from the published archive, which itself verifies against
/// `EXPECTED_SHA256` — so these are derived from the same artifact the install
/// already trusts, not from a separate download.
///
/// B15's Done-when asks for partner files to be verified against a pinned
/// manifest BEFORE EVERY SPAWN, with automatic repair on mismatch. The archive
/// hash alone cannot do that: it is checked once at install time and says
/// nothing about what is on disk at spawn time, which is exactly the window in
/// which an antivirus quarantine or a partial update corrupts a file.
const PINNED_FILES: [(&str, &str, u64); 2] = [
    (
        "titan-edge.exe",
        "a9e4a521343a1ce6800ba15178d7da403cd48ccf15e3df4adc464969dcf95e8b",
        162_912_779,
    ),
    (
        "goworkerd.dll",
        "997cd9439ed79ea22c311dcca7308604755517e15c2c3ee17ce96f41412609e6",
        177_544_704,
    ),
];

/// Files already verified in this process run, keyed by path, with the size and
/// mtime they had when they passed.
///
/// Hashing 340 MB on every spawn would make a restart cycle expensive, and the
/// supervisor can restart several times in a row. A file whose size AND mtime
/// are unchanged since it last verified has not been swapped, so re-hashing it
/// buys nothing; anything that touches the file invalidates the entry.
static VERIFIED_FILES: std::sync::Mutex<
    Option<std::collections::HashMap<PathBuf, (u64, std::time::SystemTime)>>,
> = std::sync::Mutex::new(None);

/// PURE: does this file's metadata match what it had when it last verified?
pub(crate) fn metadata_unchanged(
    recorded: Option<(u64, std::time::SystemTime)>,
    now: (u64, std::time::SystemTime),
) -> bool {
    recorded == Some(now)
}

/// Verify the pinned partner files, returning the names that are missing or do
/// not match. Empty means the tree is trustworthy.
fn unverified_pinned_files(partner_dir: &std::path::Path) -> Vec<String> {
    let mut bad = Vec::new();
    for (name, expected, expected_len) in PINNED_FILES {
        let path = partner_dir.join(name);
        let Ok(meta) = std::fs::metadata(&path) else {
            bad.push(format!("{name} is missing"));
            continue;
        };
        if meta.len() != expected_len {
            bad.push(format!(
                "{name} is {} bytes, expected {expected_len}",
                meta.len()
            ));
            continue;
        }
        let stamp = match meta.modified() {
            Ok(m) => Some((meta.len(), m)),
            Err(_) => None,
        };
        let cached = stamp.and_then(|s| {
            VERIFIED_FILES
                .lock()
                .ok()
                .and_then(|g| g.as_ref().and_then(|m| m.get(&path).copied()))
                .filter(|recorded| metadata_unchanged(Some(*recorded), s))
        });
        if cached.is_some() {
            continue;
        }
        match TitanIntegration::compute_sha256(&path) {
            Ok(actual) if actual.eq_ignore_ascii_case(expected) => {
                if let (Some(s), Ok(mut guard)) = (stamp, VERIFIED_FILES.lock()) {
                    guard.get_or_insert_with(Default::default).insert(path, s);
                }
            }
            Ok(actual) => bad.push(format!("{name} hashes to {actual}, expected {expected}")),
            Err(e) => bad.push(format!("{name} could not be read: {e}")),
        }
    }
    bad
}

/// Move a partner file that does not match its pin out of the way.
///
/// Renamed, never deleted: the displaced file is the only evidence of what was
/// actually on disk, and a quarantined-then-restored antivirus artefact is
/// exactly the thing worth keeping.
fn quarantine_unverified(partner_dir: &std::path::Path) {
    for (name, _, _) in PINNED_FILES {
        let path = partner_dir.join(name);
        if !path.exists() {
            continue;
        }
        let dest = partner_dir.join(format!("{name}.untrusted"));
        match std::fs::rename(&path, &dest) {
            Ok(()) => warn!(moved_to = ?dest, "Quarantined a Titan file that failed verification"),
            Err(e) => warn!(error = %e, file = name, "Could not quarantine a Titan file"),
        }
    }
    if let Ok(mut guard) = VERIFIED_FILES.lock() {
        if let Some(map) = guard.as_mut() {
            map.clear();
        }
    }
}

/// Remove an extracted release directory and everything inside it.
///
/// Recursive on purpose: `remove_dir` fails the moment the archive carries any
/// third file, and the leftover `titan-edge_v0.1.20_…` directory is exactly
/// what users photographed. A directory that is already gone is not an error.
async fn clear_extracted_dir(dir: &std::path::Path) {
    match tokio::fs::remove_dir_all(dir).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => warn!(error = %e, path = ?dir, "Could not clear the extracted Titan directory"),
    }
}

const DOWNLOAD_URL: &str = "https://github.com/Titannet-dao/titan-node/releases/download/v0.1.20/titan-edge_v0.1.20_246b9dd_widnows_amd64.tar.gz";
const EXPECTED_SHA256: &str = "6f37eea5cfcd6f799cd629d6e02a5636fb5c92995f73f0791ec0ff473afb558c";
const USER_AGENT: &str = concat!("FryEdgeMiner/", env!("CARGO_PKG_VERSION"));
const DAEMON_URL: &str = "https://cassini-locator.titannet.io:5000/rpc/v0";

pub struct TitanIntegration {
    pub supervisor: Arc<Mutex<Supervisor>>,
    pub log_dir: PathBuf,
}

/// Whether one titan-edge log line reports a real failure.
///
/// The health check used to flag any line merely *containing* "error", which
/// matched routine output — a summary reading `errors=0`, a URL with "error"
/// in the path, a Go `error=<nil>` field — and left the card stuck on
/// Unhealthy while the daemon was fine. Match a log LEVEL instead, and never
/// let an explicitly-zero/absent error count count as one.
fn line_indicates_error(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    // An explicit "no error" report is the opposite of a failure.
    const BENIGN: [&str; 6] = [
        "0 errors",
        "errors=0",
        "errors: 0",
        "no error",
        "error=<nil>",
        "error: <nil>",
    ];
    if BENIGN.iter().any(|b| lower.contains(b)) {
        return false;
    }
    if lower.contains("error=nil") || lower.contains("\"error\":null") {
        return false;
    }
    // Structured level fields (zap / logrus / slog).
    if lower.contains("level=error")
        || lower.contains("level=fatal")
        || lower.contains("\"level\":\"error\"")
        || lower.contains("\"level\":\"fatal\"")
    {
        return true;
    }
    // Bare level tokens: "[ERROR]", a leading "ERROR ", or " ERROR " / " FATAL ".
    line.split(|c: char| !c.is_ascii_alphabetic())
        .any(|tok| tok.eq_ignore_ascii_case("error") || tok.eq_ignore_ascii_case("fatal"))
        && line.split_whitespace().take(4).any(|w| {
            let t = w.trim_matches(|c: char| !c.is_ascii_alphabetic());
            t.eq_ignore_ascii_case("error") || t.eq_ignore_ascii_case("fatal")
        })
}

#[cfg(test)]
mod log_level_tests {
    use super::line_indicates_error;

    #[test]
    fn routine_lines_are_not_failures() {
        for line in [
            "2026-08-27T03:00:00Z INFO scheduler completed with 0 errors",
            "2026-08-27T03:00:00Z INFO sync finished errors=0 duration=1.2s",
            "2026-08-27T03:00:00Z INFO fetched https://api.example.com/v1/error-codes",
            "2026-08-27T03:00:00Z INFO rpc call returned error=<nil>",
            "2026-08-27T03:00:00Z INFO Daemon listening on 0.0.0.0:1234",
        ] {
            assert!(!line_indicates_error(line), "should be benign: {line}");
        }
    }

    #[test]
    fn real_failures_are_still_caught() {
        for line in [
            "2026-08-27T03:00:00Z ERROR failed to dial locator",
            "2026-08-27T03:00:00Z [ERROR] candidate registration rejected",
            "time=2026-08-27T03:00:00Z level=error msg=\"disk full\"",
            "{\"level\":\"fatal\",\"msg\":\"cannot open datastore\"}",
            "2026-08-27T03:00:00Z FATAL unrecoverable state",
        ] {
            assert!(line_indicates_error(line), "should be a failure: {line}");
        }
    }
}

/// BUG 8: reason shown for a dead titan-edge process, preferring the
/// specific VC++-runtime diagnosis (directly verifiable via
/// `vc_redist_dll_present`) over the generic log-tail fallback shared with
/// BUG 6 (fryvpn) / BUG 9 (mysterium) — a missing DLL means the process
/// likely never got far enough to write anything useful to its own logs.
/// BUG 3: how many consecutive failing health ticks before the card is marked
/// UNHEALTHY. titan-edge legitimately logs RPC timeouts while it is still
/// locating a scheduler, and the health loop ticks every 30 s — so a single
/// blip must not flip a working card (and trigger a restart cascade).
pub(crate) const CONSECUTIVE_FAILURES_BEFORE_UNHEALTHY: u32 = 3;

/// Consecutive failing health ticks seen so far. Reset the moment a clean tick
/// is observed, so tolerance never accumulates across unrelated incidents.
static TITAN_CONSECUTIVE_FAILURES: AtomicU32 = AtomicU32::new(0);

/// PURE: consecutive-failure gate, so the tolerance is testable without timing.
pub(crate) fn should_report_unhealthy(consecutive_failures: u32) -> bool {
    consecutive_failures >= CONSECUTIVE_FAILURES_BEFORE_UNHEALTHY
}

/// PURE: the first genuinely-failing line in a log tail, or None.
/// Reuses the existing `line_indicates_error` contract so benign lines that
/// merely contain "error" (errors=0, error=<nil>, a docs URL) stay benign.
pub(crate) fn first_error_line(log: &str) -> Option<&str> {
    log.lines().map(str::trim).find(|l| line_indicates_error(l))
}

/// How far back a titan-edge log line may be stamped and still count as a
/// reason the CURRENT tick is failing.
pub(crate) const ERROR_RECENCY_WINDOW_MINUTES: i64 = 5;

/// PURE: is this log line recent enough to describe the current state?
///
/// The tail is a LINE window (last 50), never a time window, and the failure
/// counter only resets when the whole tail is clean — so on a quiet log one
/// historical ERROR was re-counted on every tick forever, holding the card
/// UNHEALTHY (and, before the recovery exemption, restarting a live daemon)
/// long after the condition had cleared.
///
/// titan-edge stamps `2026-09-18T19:56:30.482-0500`: an offset with no colon,
/// so this is NOT rfc3339 and `parse_from_rfc3339` will not read it.
///
/// Fails OPEN: a line with no parseable timestamp counts as recent, which is
/// exactly today's behaviour for every line that is not titan-shaped.
pub(crate) fn error_line_is_recent(
    line: &str,
    now: chrono::DateTime<chrono::Utc>,
    window: chrono::Duration,
) -> bool {
    let Some(token) = line.split_whitespace().next() else {
        return true;
    };
    let Ok(stamped) = chrono::DateTime::parse_from_str(token, "%Y-%m-%dT%H:%M:%S%.f%z") else {
        return true;
    };
    now.signed_duration_since(stamped.with_timezone(&chrono::Utc)) <= window
}

/// PURE: the first genuinely-failing line that is also recent enough to be
/// describing now.
pub(crate) fn first_recent_error_line(
    log: &str,
    now: chrono::DateTime<chrono::Utc>,
    window: chrono::Duration,
) -> Option<&str> {
    log.lines()
        .map(str::trim)
        .find(|l| line_indicates_error(l) && error_line_is_recent(l, now, window))
}

/// PURE: the user-facing reason for a daemon that is running but logging
/// failures. Carries the REAL error through instead of the old fixed
/// placeholder, and explains the common connectivity case in plain language.
pub(crate) fn daemon_log_failure_reason(log: &str) -> String {
    let Some(line) = first_error_line(log) else {
        return "Titan Network: the daemon reported a problem".to_string();
    };
    let lower = line.to_lowercase();
    let is_connectivity = lower.contains("timeout")
        || lower.contains("i/o timeout")
        || lower.contains("connection refused")
        || lower.contains("no such host")
        || lower.contains("dial tcp");
    if is_connectivity {
        format!(
            "Titan Network: cannot reach the Titan scheduler — this is a network              connection problem, not a fault on this device. Titan retries              automatically. Details: {}",
            super::stderr_tail(line, 1)
        )
    } else {
        format!("Titan Network: {}", super::stderr_tail(line, 1))
    }
}

fn process_not_running_reason(vc_redist_missing: bool, stderr_tail: &str) -> String {
    if vc_redist_missing {
        return "titan-edge process is not running: missing VC++ 2015-2022 x64 runtime \
                (VCRUNTIME140.dll / MSVCP140.dll not found) — install the Visual C++ \
                Redistributable to fix this"
            .to_string();
    }
    if stderr_tail == "no error output" {
        "titan-edge process is not running".to_string()
    } else {
        format!("titan-edge process is not running: {stderr_tail}")
    }
}

impl TitanIntegration {
    /// The real install. `trigger` decides whether the VC++ redistributable
    /// installer may raise a UAC prompt: only a user gesture ever may (B3).
    async fn install_inner(&self, trigger: crate::elevation_gate::ElevationTrigger) -> Result<()> {
        let binary = Self::binary_path();
        let partner_dir = Self::partner_dir();
        if Self::install_is_complete(&partner_dir) {
            info!(path = ?binary, "titan-edge is installed and complete");
            return Ok(());
        }
        if binary.exists() {
            // Exe present, DLL missing: a half-install that used to report
            // "already present" forever. Clear the exe so the reinstall below
            // is a full one rather than a no-op.
            warn!(path = ?binary, "titan-edge is present but goworkerd.dll is missing — reinstalling");
            let _ = tokio::fs::remove_file(&binary).await;
        }

        info!("Installing Titan Network from GitHub release");

        tokio::fs::create_dir_all(&partner_dir).await?;

        let archive_path = partner_dir.join("titan-edge.tar.gz");
        let extracted_dir = partner_dir.join("titan-edge_v0.1.20_246b9dd_widnows_amd64");

        // Clear the residue a previously-killed extraction left behind, so a
        // retry starts from a known state instead of unpacking over it.
        let _ = tokio::fs::remove_file(&archive_path).await;
        clear_extracted_dir(&extracted_dir).await;

        // Download the archive
        download_file_with_options(DOWNLOAD_URL, &archive_path, USER_AGENT, None).await?;
        info!(archive = ?archive_path, "Downloaded Titan release archive");

        // Verify SHA256
        let computed_sha256 = Self::compute_sha256(&archive_path)?;
        if computed_sha256 != EXPECTED_SHA256 {
            let _ = tokio::fs::remove_file(&archive_path).await;
            anyhow::bail!(
                "SHA256 mismatch for titan-edge.tar.gz: expected {}, got {}",
                EXPECTED_SHA256,
                computed_sha256
            );
        }
        info!("SHA256 verification passed");

        // Extract using tar command (Windows 10+ includes bsdtar)
        let output = crate::supervisor::platform::command("tar")
            .args([
                "-xzf",
                &archive_path.to_string_lossy(),
                "-C",
                &partner_dir.to_string_lossy(),
            ])
            .output_bounded(EXTRACT_TIMEOUT)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(stderr = %stderr, "tar extraction failed");
            anyhow::bail!("Failed to extract titan-edge.tar.gz: {}", stderr);
        }
        info!("Extracted archive");

        // The archive contains titan-edge_v0.1.20_246b9dd_widnows_amd64/ with titan-edge.exe and goworkerd.dll
        // Move both files to the parent directory
        if extracted_dir.exists() {
            let exe_in_subdir = extracted_dir.join("titan-edge.exe");
            let dll_in_subdir = extracted_dir.join("goworkerd.dll");

            if exe_in_subdir.exists() {
                tokio::fs::rename(&exe_in_subdir, &binary).await?;
                info!(path = ?binary, "Moved titan-edge.exe to partner dir");
            }

            if dll_in_subdir.exists() {
                tokio::fs::rename(&dll_in_subdir, &Self::dll_path()).await?;
                info!(path = ?Self::dll_path(), "Moved goworkerd.dll to partner dir");
            }

            clear_extracted_dir(&extracted_dir).await;
        }

        // Clean up archive
        let _ = tokio::fs::remove_file(&archive_path).await;

        // BUG 8: titan-edge.exe needs the VC++ 2015-2022 x64 runtime to even
        // launch. Install it now (one elevated prompt) rather than waiting
        // for the daemon to fail fast on first start — best-effort: a
        // decline/failure here must not fail the whole integration install,
        // since `health_check()` still reports the concrete reason if it
        // turns out to be missing at start time.
        if vc_redist_missing() {
            match install_vc_redist_elevated(trigger).await {
                Ok(VcRedistInstallOutcome::Installed) => {}
                Ok(VcRedistInstallOutcome::StillInstalling) => {
                    // H2 review fix: not a failure — health_check() re-checks
                    // vc_redist_missing() on the next tick once the process
                    // has had more time to finish.
                    info!("VC++ redist install still in progress — will confirm on a later health check");
                }
                // `install_vc_redist_elevated` currently always converts a
                // Failed outcome into an Err before returning to the caller
                // (see its own match), but VcRedistInstallOutcome is part of
                // the public return type — handle this defensively the same
                // as Err rather than relying on that internal detail.
                Ok(VcRedistInstallOutcome::Failed(code)) => {
                    warn!(exit_code = ?code, "VC++ redist install declined or failed — titan-edge may fail to start until it is installed manually");
                }
                Err(e) => {
                    warn!(error = %e, "VC++ redist install declined or failed — titan-edge may fail to start until it is installed manually");
                }
            }
        }

        // Success used to be logged and returned with zero verification, so
        // an install that moved nothing still reported Ok(()) and the failure
        // only surfaced one step later, at start(), as "binary not found".
        if !Self::install_is_complete(&partner_dir) {
            anyhow::bail!(
                "Titan install finished but {} / {} are missing",
                binary.display(),
                Self::dll_path().display()
            );
        }

        info!(binary = ?binary, "Titan Network installed successfully");
        Ok(())
    }

    fn partner_dir() -> PathBuf {
        partners_base_dir().join("titan")
    }

    fn binary_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        return Self::partner_dir().join("titan-edge.exe");
        #[cfg(not(target_os = "windows"))]
        return Self::partner_dir().join("titan-edge");
    }

    fn dll_path() -> PathBuf {
        Self::partner_dir().join("goworkerd.dll")
    }

    /// PURE: is the partner directory a COMPLETE titan install?
    ///
    /// The entry guard used to check only titan-edge.exe, so a half-install —
    /// the exe moved out of the extracted subdirectory but goworkerd.dll left
    /// behind — reported "already present", returned Ok(()) and was never
    /// repaired. titan-edge.exe cannot run without that DLL.
    pub(crate) fn install_is_complete(partner_dir: &std::path::Path) -> bool {
        let exe = if cfg!(target_os = "windows") {
            partner_dir.join("titan-edge.exe")
        } else {
            partner_dir.join("titan-edge")
        };
        exe.exists() && partner_dir.join("goworkerd.dll").exists()
    }

    fn compute_sha256(path: &PathBuf) -> Result<String> {
        use sha2::{Digest, Sha256};
        use std::io::Read;

        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }
}

#[async_trait]
impl Integration for TitanIntegration {
    fn id(&self) -> &str {
        "titan"
    }

    fn display_name(&self) -> &str {
        "Titan Network"
    }

    async fn install(&self) -> Result<()> {
        self.install_inner(crate::elevation_gate::ElevationTrigger::Automatic)
            .await
    }

    /// B3: only a real click may raise UAC.
    async fn install_for_user(&self) -> Result<()> {
        self.install_inner(crate::elevation_gate::ElevationTrigger::UserClick)
            .await
    }

    async fn start(&self) -> Result<()> {
        let binary = Self::binary_path();
        if !binary.exists() {
            anyhow::bail!("titan-edge binary not found at {}", binary.display());
        }

        // B15: verify the pinned files BEFORE handing anything to the loader.
        // Offloaded, because hashing is blocking work and the bound around
        // start() has to be able to fire. One repair attempt, then refuse
        // rather than spawn an image we know is wrong.
        let partner_dir = Self::partner_dir();
        let check_dir = partner_dir.clone();
        let bad = tokio::task::spawn_blocking(move || unverified_pinned_files(&check_dir))
            .await
            .map_err(|e| anyhow::anyhow!("Titan verification task panicked: {e}"))?;
        if !bad.is_empty() {
            warn!(problems = ?bad, "Titan partner files failed verification — repairing");
            let repair_dir = partner_dir.clone();
            tokio::task::spawn_blocking(move || quarantine_unverified(&repair_dir))
                .await
                .map_err(|e| anyhow::anyhow!("Titan quarantine task panicked: {e}"))?;
            self.install().await?;
            let recheck_dir = partner_dir.clone();
            let still_bad =
                tokio::task::spawn_blocking(move || unverified_pinned_files(&recheck_dir))
                    .await
                    .map_err(|e| anyhow::anyhow!("Titan verification task panicked: {e}"))?;
            if !still_bad.is_empty() {
                anyhow::bail!(
                    "Titan Network files could not be restored to their pinned versions: {}",
                    still_bad.join("; ")
                );
            }
        }

        let binary_str = binary.to_string_lossy().to_string();
        let args = [
            "daemon",
            "start",
            "--init",
            &format!("--url={}", DAEMON_URL),
        ];

        {
            let mut sup = self.supervisor.lock().unwrap();
            sup.start_integration("titan", &binary_str, &args)
                .map_err(|e| anyhow::anyhow!("Failed to spawn titan-edge daemon: {}", e))?;
        }

        info!("Titan Network daemon started");
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        {
            let mut sup = self.supervisor.lock().unwrap();
            sup.stop_integration("titan")
                .map_err(|e| anyhow::anyhow!("Failed to stop titan-edge daemon: {}", e))?;
        }
        info!("Titan Network daemon stopped");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        // Check process alive
        let process_alive = {
            let mut sup = self.supervisor.lock().unwrap();
            matches!(sup.get_status("titan"), HealthStatus::Healthy)
        };

        if !process_alive {
            // BUG 8: same defect class as BUG 6/9 — a bare `Stopped` for an
            // enabled-but-dead integration renders as "Starting" forever
            // with no reason. titan-edge is a cgo binary, so a missing VC++
            // runtime is the single most common cause and is directly
            // verifiable (no exit-code plumbing needed).
            // B15: before blaming the usual suspects, ask whether Windows
            // REFUSED to load the image. A Smart App Control / WDAC block kills
            // the child instantly and leaves nothing in the logs (spawn_full
            // truncates both on every attempt), so without this the reason is
            // wrong AND the health loop keeps respawning through a refusal it
            // can never satisfy. The returned message carries the
            // awaits-user-action marker, which is what stops that loop.
            if let Some(blocked) = super::code_integrity::recent_block(&Self::binary_path())
                .or_else(|| super::code_integrity::recent_block(&Self::dll_path()))
            {
                return HealthStatus::Unhealthy(blocked);
            }
            let stderr_path = self.log_dir.join("titan").join("titan_stderr.log");
            let stderr_content = tokio::fs::read_to_string(&stderr_path)
                .await
                .unwrap_or_default();
            let tail = super::stderr_tail(&stderr_content, 3);
            return HealthStatus::Unhealthy(process_not_running_reason(vc_redist_missing(), &tail));
        }

        // Read both log files (stdout and stderr)
        let stdout_path = self.log_dir.join("titan").join("titan_stdout.log");
        let stderr_path = self.log_dir.join("titan").join("titan_stderr.log");

        let stdout_content = tokio::fs::read_to_string(&stdout_path)
            .await
            .unwrap_or_default();
        let stderr_content = tokio::fs::read_to_string(&stderr_path)
            .await
            .unwrap_or_default();

        // Combine recent tail (last 50 lines of each)
        let recent_lines: Vec<String> = stdout_content
            .lines()
            .chain(stderr_content.lines())
            .rev()
            .take(50)
            .map(|l| l.to_string())
            .collect();

        // BUG 3: carry the REAL error through, and tolerate transients.
        // The old code collapsed every matched line to a fixed placeholder and
        // flipped the card on the very first tick.
        let combined = recent_lines.join(
            "
",
        );
        if first_recent_error_line(
            &combined,
            chrono::Utc::now(),
            chrono::Duration::minutes(ERROR_RECENCY_WINDOW_MINUTES),
        )
        .is_some()
        {
            let failures = TITAN_CONSECUTIVE_FAILURES.fetch_add(1, Ordering::Relaxed) + 1;
            if should_report_unhealthy(failures) {
                return HealthStatus::Unhealthy(daemon_log_failure_reason(&combined));
            }
            // Not yet persistent: report the transient state honestly rather
            // than claiming health we cannot demonstrate.
            return HealthStatus::Starting;
        }
        TITAN_CONSECUTIVE_FAILURES.store(0, Ordering::Relaxed);

        // Check for daemon startup markers (conservative: process alive + no errors = Starting, look for running marker)
        let is_running = recent_lines
            .iter()
            .any(|l| l.contains("Daemon") || l.contains("edge") || l.contains("listening"));

        if is_running {
            HealthStatus::Healthy
        } else {
            // Process up but daemon not yet fully started
            HealthStatus::Starting
        }
    }

    async fn check_update(&self) -> Result<Option<String>> {
        // TODO: partner binary version check via /versions/titan
        Ok(None)
    }

    fn installed_version(&self) -> Option<String> {
        // G4 finding 7: this checked ONLY titan-edge.exe, and every production
        // caller of install() gates on it — the toggle, the boot recovery pass
        // and the Docker watcher. So the half-install repair added for B13 was
        // unreachable: with the exe present and goworkerd.dll missing or
        // quarantined (the reported "Bad Image … goworkerd.dll" case) this
        // returned Some, install() was skipped, start() spawned anyway, the
        // loader failed, and the health loop restarted the same broken tree
        // forever with no in-app repair.
        //
        // Existence checks only, deliberately: this runs inside the registry
        // snapshot on a 30 s poll, so it must stay cheap. The sha256 work lives
        // in start(), once per spawn.
        if Self::install_is_complete(&Self::partner_dir()) {
            Some("v0.1.20".into())
        } else {
            None
        }
    }

    fn collect_poc_data(&self) -> PocGateData {
        // Sync — supervisor status only
        let status = {
            let mut sup = self.supervisor.lock().unwrap();
            sup.get_status("titan")
        };
        PocGateData {
            poa: matches!(status, HealthStatus::Healthy),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod bug8_vc_redist_tests {
    use super::*;

    fn tmp_system_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fem-titan-test-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("System32")).unwrap();
        dir
    }

    #[test]
    fn the_marker_dll_present_means_the_runtime_is_installed() {
        let root = tmp_system_root("present");
        std::fs::write(
            root.join("System32").join(VC_REDIST_MARKER_DLL),
            b"fake dll",
        )
        .unwrap();
        assert!(vc_redist_dll_present(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_marker_dll_means_the_runtime_is_not_installed() {
        let root = tmp_system_root("missing");
        assert!(!vc_redist_dll_present(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    // --- process_not_running_reason (pure) ----------------------------------

    #[test]
    fn a_missing_runtime_gets_a_specific_actionable_reason_regardless_of_log_content() {
        let reason = process_not_running_reason(true, "no error output");
        assert!(reason.contains("VC++ 2015-2022"));
        assert!(reason.contains("VCRUNTIME140") || reason.contains("MSVCP140"));
    }

    #[test]
    fn a_missing_runtime_reason_takes_priority_over_a_generic_log_tail() {
        // Even if SOME stderr happened to be captured, the VC++ diagnosis is
        // the more actionable and more likely correct root cause for a cgo
        // binary that failed to launch at all.
        let reason = process_not_running_reason(true, "some unrelated log line");
        assert!(reason.contains("VC++ 2015-2022"));
        assert!(!reason.contains("some unrelated log line"));
    }

    #[test]
    fn a_present_runtime_falls_back_to_the_generic_stderr_tail_reason() {
        let reason = process_not_running_reason(false, "panic: disk full");
        assert!(reason.contains("panic: disk full"));
        assert!(!reason.contains("VC++"));
    }

    #[test]
    fn a_present_runtime_with_no_log_output_still_gets_a_non_empty_reason() {
        let reason = process_not_running_reason(false, "no error output");
        assert_eq!(reason, "titan-edge process is not running");
    }
}

/// H2 review fix: the elevated VC++ redist install's timeout budget and
/// timeout-vs-failure outcome mapping.
#[cfg(test)]
mod bug8_h2_vc_redist_timeout_tests {
    use super::*;

    #[test]
    fn the_install_timeout_is_materially_longer_than_the_generic_probe_timeout() {
        // The pre-fix bug: reusing PROBE_TIMEOUT (20s) for a real
        // Microsoft installer, which routinely takes well over 20s even
        // in quiet mode.
        assert!(
            VC_REDIST_INSTALL_TIMEOUT > crate::supervisor::platform::PROBE_TIMEOUT,
            "VC_REDIST_INSTALL_TIMEOUT must be materially longer than the generic 20s probe timeout"
        );
        assert_eq!(
            VC_REDIST_INSTALL_TIMEOUT,
            std::time::Duration::from_secs(600),
            "bounded at 10 minutes"
        );
    }

    #[test]
    fn a_timeout_is_reported_as_still_installing_never_as_a_failure() {
        // The pre-fix bug: output_bounded's TimedOut only kills the OUTER
        // unelevated wrapper — the real elevated installer runs in a
        // separate security context the kill cannot reach, so it may well
        // succeed moments later. Treating this as a hard failure defeats
        // BUG 8's auto-remediation for the realistic "slow but working" case.
        assert_eq!(
            vc_redist_install_outcome(true, false, None),
            VcRedistInstallOutcome::StillInstalling
        );
        // Even a would-be-successful exit code must not override a timeout
        // — a timeout means we never actually observed it.
        assert_eq!(
            vc_redist_install_outcome(true, true, Some(0)),
            VcRedistInstallOutcome::StillInstalling
        );
    }

    #[test]
    fn a_clean_success_within_budget_is_installed() {
        assert_eq!(
            vc_redist_install_outcome(false, true, Some(0)),
            VcRedistInstallOutcome::Installed
        );
    }

    #[test]
    fn exit_code_3010_restart_required_is_still_installed() {
        assert_eq!(
            vc_redist_install_outcome(false, false, Some(3010)),
            VcRedistInstallOutcome::Installed
        );
    }

    #[test]
    fn a_genuine_non_timeout_failure_is_failed_with_its_exit_code() {
        assert_eq!(
            vc_redist_install_outcome(false, false, Some(1603)),
            VcRedistInstallOutcome::Failed(Some(1603))
        );
        assert_eq!(
            vc_redist_install_outcome(false, false, None),
            VcRedistInstallOutcome::Failed(None)
        );
    }
}

/// BUG 3 (georgeparis): "Titan Network UNHEALTHY — RPC timeout to
/// test23-scheduler.titannet.io:3456".
///
/// Measured from FryStation during recon — the endpoint is NOT the problem:
///   test23-scheduler.titannet.io -> 47.76.123.118   TCP 3456 : reachable
///   cassini-locator.titannet.io  -> 8.211.33.28     TCP 5000 : reachable  (what FEM uses)
///   locator.titannet.io          -> 39.108.214.29   TCP 5000 : CLOSED
///   mainnet-locator.titannet.io  -> NXDOMAIN
/// `test23-scheduler` is handed to titan-edge by the locator at runtime and
/// appears nowhere in this repo, and the only alternative locator does not
/// answer — so there is no endpoint to switch to.
///
/// The actual defect: a matched error line collapsed to the fixed string
/// "Error detected in Titan daemon logs", throwing away the one piece of
/// information the user needed, even though `stderr_tail` was already imported
/// and used two branches above.
#[cfg(test)]
mod bug3_daemon_error_tests {
    use super::*;

    #[test]
    fn the_real_daemon_error_reaches_the_user_instead_of_a_fixed_string() {
        let log = "2026-09-14 10:00:01 INFO  starting edge node\n\
                   2026-09-14 10:00:31 ERROR rpc timeout: dial tcp 47.76.123.118:3456: i/o timeout\n";
        let reason = daemon_log_failure_reason(log);
        assert!(
            reason.contains("3456") || reason.contains("timeout"),
            "the actual failure must survive into the reason, got: {reason}"
        );
        assert_ne!(
            reason, "Error detected in Titan daemon logs",
            "BUG 3: the fixed placeholder string discards the real error"
        );
    }

    /// A scheduler/RPC timeout is a specific, actionable condition — say so in
    /// plain language rather than surfacing a Go error verbatim.
    #[test]
    fn a_scheduler_timeout_is_explained_in_plain_language() {
        let log = "ERROR rpc timeout: dial tcp 47.76.123.118:3456: i/o timeout\n";
        let reason = daemon_log_failure_reason(log);
        let lower = reason.to_lowercase();
        assert!(
            lower.contains("titan") && (lower.contains("network") || lower.contains("connection")),
            "a timeout should read as a connectivity problem, got: {reason}"
        );
    }

    #[test]
    fn a_log_with_no_error_lines_produces_no_reason() {
        let log = "INFO starting edge node\nINFO listening on 0.0.0.0:1234\n";
        assert_eq!(first_error_line(log), None);
    }

    /// Benign lines that merely contain the substring "error" must not trip the
    /// detector — this is the pre-existing `line_indicates_error` contract and
    /// it must keep holding through the new path.
    #[test]
    fn benign_lines_containing_the_word_error_are_not_failures() {
        for benign in [
            "INFO  errors=0 warnings=0",
            "INFO  error=<nil>",
            "INFO  see https://docs.titannet.io/troubleshooting/error-codes",
        ] {
            assert_eq!(
                first_error_line(benign),
                None,
                "{benign:?} must not be a failure"
            );
        }
    }

    /// A single transient failure must not flip a card that was healthy — the
    /// daemon legitimately logs RPC timeouts while it is finding a scheduler.
    #[test]
    fn one_transient_failure_does_not_flip_the_card() {
        assert!(!should_report_unhealthy(1));
        assert!(!should_report_unhealthy(
            CONSECUTIVE_FAILURES_BEFORE_UNHEALTHY - 1
        ));
    }

    #[test]
    fn a_persistent_failure_is_still_reported() {
        assert!(should_report_unhealthy(
            CONSECUTIVE_FAILURES_BEFORE_UNHEALTHY
        ));
        assert!(should_report_unhealthy(
            CONSECUTIVE_FAILURES_BEFORE_UNHEALTHY + 5
        ));
    }
}

#[cfg(test)]
#[path = "titan_layout_tests.rs"]
mod titan_layout_tests;

#[cfg(test)]
#[path = "titan_recency_tests.rs"]
mod titan_recency_tests;
