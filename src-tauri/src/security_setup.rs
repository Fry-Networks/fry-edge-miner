//! BUG 1/2 (part d): one-time elevated hardening so Windows Defender never
//! quarantines an in-place update mid-flight and the frynode firewall rule
//! exists without a separate UAC prompt per integration.
//!
//! Runs once per FEM version (recorded in `FemConfig::hardening_applied_version`)
//! — on first launch after install, and again before each update to a new
//! version — via a SINGLE elevated PowerShell process (`Start-Process -Verb
//! RunAs -Wait`), mirroring the exact elevation pattern already proven in
//! `integrations::firewall::ensure_program_rules`. Declining the UAC prompt
//! is not retried every launch; the manual command is logged and surfaced so
//! the operator can run it themselves.

use std::path::Path;

use tracing::{info, warn};

use crate::integrations::firewall;
use crate::supervisor::platform::BoundedOutput;

/// Whether hardening needs to (re-)run for `current_version`, given the
/// version last recorded as successfully hardened. Pure — no I/O, no UAC.
pub fn should_run_hardening(recorded_version: Option<&str>, current_version: &str) -> bool {
    recorded_version != Some(current_version)
}

/// PowerShell single-quote escaping (double the `'`), same helper as
/// `firewall::ensure_program_rules` uses for the identical reason (paths
/// containing quotes).
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Build the elevated inner script: Defender path + process exclusions for
/// this install, plus the frynode firewall rule — combined so both land
/// under the SAME UAC prompt rather than two separate ones. Pure/testable:
/// takes plain data in, returns a script string, no process spawned.
pub fn build_hardening_script(install_dir: &Path, exe_names: &[&str], frynode_path: &Path) -> String {
    let install_dir_str = install_dir.to_string_lossy().to_string();
    let exclusion_process_list = exe_names
        .iter()
        .map(|n| ps_quote(n))
        .collect::<Vec<_>>()
        .join(",");

    let defender_cmd = format!(
        "Add-MpPreference -ExclusionPath {} -ErrorAction SilentlyContinue; \
         Add-MpPreference -ExclusionProcess {} -ErrorAction SilentlyContinue",
        ps_quote(&install_dir_str),
        exclusion_process_list
    );

    let firewall_cmds = firewall::reconcile_commands("FEM-FryNode", &frynode_path.to_string_lossy())
        .into_iter()
        .map(|argv| {
            let quoted: Vec<String> = argv
                .iter()
                .map(|a| {
                    if let Some(prog) = a.strip_prefix("program=") {
                        format!("program={}", ps_quote(prog))
                    } else {
                        a.clone()
                    }
                })
                .collect();
            format!("netsh {}", quoted.join(" "))
        })
        .collect::<Vec<_>>()
        .join("; ");

    format!("{defender_cmd}; {firewall_cmds}")
}

/// v0.4.29 canary fix. Build the OUTER (unelevated) PowerShell wrapper that
/// launches `inner` elevated via `Start-Process -Verb RunAs -Wait -PassThru`.
/// Pure/testable.
///
/// Root cause of the canary defect: `Start-Process -Verb RunAs` raises a
/// NON-TERMINATING error when UAC is declined/cancelled/unavailable (default
/// `$ErrorActionPreference = 'Continue'`), so execution continued past that
/// line with `$p` left as `$null`. `$p.ExitCode` on a `$null` `$p` silently
/// evaluates to `$null`, and `exit $null` exits with code **0** — a DECLINED
/// UAC prompt was therefore indistinguishable from a successful silent
/// install. Confirmed live on the FryStation canary: `hardening_applied_version`
/// was recorded as "0.4.28", the FEM-FryNode firewall rule does not exist,
/// and no UAC prompt was ever answered (operator AFK).
///
/// `$ErrorActionPreference = 'Stop'` promotes that error to terminating
/// (caught below, exit 2); the explicit `$null` guard (exit 3) is defense in
/// depth for any other path that could leave `$p` unset.
fn build_outer_elevation_script(inner: &str) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'; try {{ $p = Start-Process -FilePath powershell -ArgumentList '-NoProfile','-Command',\"{}\" -Verb RunAs -Wait -PassThru; if ($null -eq $p) {{ exit 3 }}; exit $p.ExitCode }} catch {{ exit 2 }}",
        inner.replace('"', "`\"")
    )
}

/// v0.4.29 canary fix, pure & testable: what to report given the outer
/// script's raw exit code and whether the frynode firewall rule is
/// GENUINELY present afterward. Exit 0 is necessary but not sufficient —
/// `rule_present` is the ground-truth check that catches a declined UAC
/// even via some exit-0 path this fix didn't anticipate (defense in depth
/// beyond the script-level fix above).
pub(crate) fn hardening_outcome(exit_code: Option<i32>, rule_present: bool) -> Result<(), String> {
    match exit_code {
        Some(0) if rule_present => Ok(()),
        Some(0) => Err(
            "hardening script exited 0 but the frynode firewall rule is not present — \
             treating as failed so it retries next launch/update"
                .to_string(),
        ),
        Some(2) => Err("hardening script raised a terminating error (UAC declined/cancelled or Start-Process failed)".to_string()),
        Some(3) => Err("hardening script's Start-Process returned no process handle (UAC declined/cancelled)".to_string()),
        Some(code) => Err(format!("hardening script exited {code}")),
        None => Err("hardening script did not report an exit code (terminated by signal?)".to_string()),
    }
}

/// Run the combined hardening script through ONE elevated PowerShell process.
/// Best-effort: a UAC decline or Defender-policy lockout (e.g. tamper
/// protection, or a managed/enterprise machine) must not block startup or
/// the update — the caller logs + surfaces the manual command and moves on.
/// Only returns `Ok` when the firewall rule is VERIFIED present afterward
/// (v0.4.29 canary fix) — a bare successful exit code is not trusted alone.
pub(crate) fn run_hardening_elevated(install_dir: &Path, exe_names: &[&str], frynode_path: &Path) -> anyhow::Result<()> {
    let inner = build_hardening_script(install_dir, exe_names, frynode_path);
    let outer = build_outer_elevation_script(&inner);

    let out = crate::supervisor::platform::command("powershell")
        .args(["-NoProfile", "-Command", &outer])
        .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)?;

    // v0.4.29 canary fix: verify the rule ACTUALLY landed before trusting
    // the exit code — see `hardening_outcome`'s docs.
    let rule_present = firewall::current_rule_program("FEM-FryNode").is_some();

    match hardening_outcome(out.status.code(), rule_present) {
        Ok(()) => {
            info!("Defender exclusions + frynode firewall rule applied (elevated, one-time)");
            Ok(())
        }
        Err(reason) => {
            warn!(
                code = out.status.code(),
                rule_present,
                reason = %reason,
                "Hardening setup declined or failed — continuing unhardened, will retry next launch/update"
            );
            anyhow::bail!("hardening setup failed: {reason}")
        }
    }
}

/// The manual command surfaced when the elevated attempt is declined/fails,
/// so the operator can run it themselves from an admin PowerShell.
pub fn manual_hardening_command(install_dir: &Path, exe_names: &[&str]) -> String {
    let install_dir_str = install_dir.to_string_lossy().to_string();
    let exclusion_process_list = exe_names
        .iter()
        .map(|n| ps_quote(n))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "Add-MpPreference -ExclusionPath {} ; Add-MpPreference -ExclusionProcess {}",
        ps_quote(&install_dir_str),
        exclusion_process_list
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn hardening_reruns_when_no_version_is_recorded_yet() {
        assert!(should_run_hardening(None, "0.4.28"));
    }

    #[test]
    fn hardening_reruns_after_a_version_bump() {
        assert!(should_run_hardening(Some("0.4.27"), "0.4.28"));
    }

    #[test]
    fn hardening_is_skipped_once_recorded_for_the_current_version() {
        assert!(!should_run_hardening(Some("0.4.28"), "0.4.28"));
    }

    #[test]
    fn the_script_excludes_the_install_dir_and_both_exe_names() {
        let install_dir = PathBuf::from(r"C:\Users\x\AppData\Local\Fry Edge Miner");
        let frynode = install_dir.join("resources").join("frynode.exe");
        let script = build_hardening_script(&install_dir, &["fry-edge-miner.exe", "frynode.exe"], &frynode);
        assert!(script.contains("Add-MpPreference -ExclusionPath"));
        assert!(script.contains("Fry Edge Miner"));
        assert!(script.contains("fry-edge-miner.exe"));
        assert!(script.contains("frynode.exe"));
    }

    #[test]
    fn the_script_also_reconciles_the_frynode_firewall_rule_in_the_same_pass() {
        let install_dir = PathBuf::from(r"C:\Users\x\AppData\Local\Fry Edge Miner");
        let frynode = install_dir.join("resources").join("frynode.exe");
        let script = build_hardening_script(&install_dir, &["fry-edge-miner.exe"], &frynode);
        assert!(script.contains("netsh advfirewall firewall"));
        assert!(script.contains("FEM-FryNode"));
    }

    #[test]
    fn a_path_containing_a_single_quote_is_escaped_for_powershell() {
        let install_dir = PathBuf::from(r"C:\Users\O'Brien\AppData\Local\Fry Edge Miner");
        let frynode = install_dir.join("resources").join("frynode.exe");
        let script = build_hardening_script(&install_dir, &["fry-edge-miner.exe"], &frynode);
        assert!(script.contains("O''Brien"), "single quote must be doubled for PowerShell: {script}");
    }

    #[test]
    fn the_manual_command_mirrors_the_elevated_scripts_exclusions() {
        let install_dir = PathBuf::from(r"C:\Users\x\AppData\Local\Fry Edge Miner");
        let cmd = manual_hardening_command(&install_dir, &["fry-edge-miner.exe", "frynode.exe"]);
        assert!(cmd.contains("Add-MpPreference -ExclusionPath"));
        assert!(cmd.contains("fry-edge-miner.exe"));
        assert!(cmd.contains("frynode.exe"));
    }

    // --- v0.4.29 canary fix: outer elevation script -------------------------

    #[test]
    fn the_outer_script_promotes_start_process_errors_to_terminating() {
        // The canary root cause: without this, a declined/cancelled UAC
        // prompt raises a NON-terminating error and execution continues
        // past the Start-Process line with $p left $null.
        let script = build_outer_elevation_script("Write-Output test");
        assert!(
            script.contains("$ErrorActionPreference = 'Stop'"),
            "must promote non-terminating errors (declined UAC) to terminating: {script}"
        );
    }

    #[test]
    fn the_outer_script_catches_the_promoted_error_with_a_distinct_exit_code() {
        let script = build_outer_elevation_script("Write-Output test");
        assert!(script.contains("try {"), "must wrap Start-Process in try/catch: {script}");
        assert!(script.contains("catch {"), "must catch the promoted terminating error: {script}");
        assert!(script.contains("exit 2"), "catch block must exit with a distinct non-zero code: {script}");
    }

    #[test]
    fn the_outer_script_guards_against_a_null_process_handle() {
        // Defense in depth beyond $ErrorActionPreference: if $p is ever
        // left $null by some other path, `exit $p.ExitCode` on a $null $p
        // silently evaluates to `exit $null`, which PowerShell treats as
        // exit code 0 — the exact canary defect (declined UAC read as
        // success). This guard must fire BEFORE `exit $p.ExitCode`.
        let script = build_outer_elevation_script("Write-Output test");
        assert!(
            script.contains("if ($null -eq $p) { exit 3 }"),
            "must guard against a null $p before reading $p.ExitCode: {script}"
        );
        let null_guard_pos = script.find("if ($null -eq $p)").expect("null guard must be present");
        let exit_code_read_pos = script.find("exit $p.ExitCode").expect("must still read $p.ExitCode on the happy path");
        assert!(
            null_guard_pos < exit_code_read_pos,
            "the null guard must run BEFORE exit $p.ExitCode, not after: {script}"
        );
    }

    #[test]
    fn the_outer_script_still_embeds_the_inner_hardening_script() {
        let script = build_outer_elevation_script("Add-MpPreference -ExclusionPath 'C:\\test'");
        assert!(script.contains("Add-MpPreference -ExclusionPath"));
    }

    // --- v0.4.29 canary fix: hardening_outcome (pure) -----------------------

    #[test]
    fn exit_0_with_the_rule_present_is_success() {
        assert_eq!(hardening_outcome(Some(0), true), Ok(()));
    }

    #[test]
    fn exit_0_with_the_rule_absent_is_the_exact_canary_defect_and_must_fail() {
        // This is the exact scenario observed live: exit code 0, but the
        // firewall rule was never created because UAC was declined and the
        // old script silently treated that as success.
        assert!(
            hardening_outcome(Some(0), false).is_err(),
            "exit 0 with the rule absent must be treated as a failure, not recorded as success"
        );
    }

    #[test]
    fn the_new_catch_exit_code_is_a_failure_regardless_of_rule_state() {
        assert!(hardening_outcome(Some(2), true).is_err());
        assert!(hardening_outcome(Some(2), false).is_err());
    }

    #[test]
    fn the_new_null_guard_exit_code_is_a_failure_regardless_of_rule_state() {
        assert!(hardening_outcome(Some(3), true).is_err());
        assert!(hardening_outcome(Some(3), false).is_err());
    }

    #[test]
    fn a_missing_exit_code_is_a_failure() {
        assert!(hardening_outcome(None, true).is_err());
        assert!(hardening_outcome(None, false).is_err());
    }

    #[test]
    fn any_other_nonzero_exit_code_is_a_failure() {
        assert!(hardening_outcome(Some(1), true).is_err());
        assert!(hardening_outcome(Some(1603), false).is_err());
    }
}
