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

/// Run the combined hardening script through ONE elevated PowerShell process.
/// Best-effort: a UAC decline or Defender-policy lockout (e.g. tamper
/// protection, or a managed/enterprise machine) must not block startup or
/// the update — the caller logs + surfaces the manual command and moves on.
pub(crate) fn run_hardening_elevated(install_dir: &Path, exe_names: &[&str], frynode_path: &Path) -> anyhow::Result<()> {
    let inner = build_hardening_script(install_dir, exe_names, frynode_path);
    let outer = format!(
        "$p = Start-Process -FilePath powershell -ArgumentList '-NoProfile','-Command',\"{}\" -Verb RunAs -Wait -PassThru; exit $p.ExitCode",
        inner.replace('"', "`\"")
    );

    let out = crate::supervisor::platform::command("powershell")
        .args(["-NoProfile", "-Command", &outer])
        .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)?;

    if out.status.success() {
        info!("Defender exclusions + frynode firewall rule applied (elevated, one-time)");
        Ok(())
    } else {
        warn!(
            code = out.status.code(),
            "Hardening setup declined or failed (UAC declined / Defender policy) — continuing unhardened"
        );
        anyhow::bail!("hardening setup failed (exit {:?})", out.status.code())
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
}
