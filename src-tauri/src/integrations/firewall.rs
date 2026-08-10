//! Windows Firewall rule management for partner binaries (v0.4.8).
//!
//! Squirrel-updated partner apps (OlostepBrowser) change their install path on
//! every self-update (`app-X.Y.Z`), so Windows re-prompts the firewall dialog
//! at every launch. FEM pre-creates allow rules for the exact binary path at
//! integration start, refreshing them when the path changes.

use std::path::Path;

use anyhow::Result;
use tracing::{info, warn};

pub const OLOSTEP_RULE_NAME: &str = "FEM-OlostepBrowser";

/// Parse the `Program:` line out of `netsh advfirewall firewall show rule
/// name=<n> verbose` output. Returns the bound program path, lowercased.
pub(crate) fn parse_rule_program(netsh_output: &str) -> Option<String> {
    netsh_output.lines().find_map(|l| {
        let t = l.trim();
        t.strip_prefix("Program:")
            .map(|v| v.trim().to_lowercase())
            .filter(|v| !v.is_empty())
    })
}

/// The netsh commands (argv form) that reconcile the rule set for `program`:
/// delete any stale rules of this name, then add inbound + outbound allow
/// rules bound to the exact binary path. Pure — unit tested.
pub(crate) fn reconcile_commands(rule_name: &str, program: &str) -> Vec<Vec<String>> {
    let name_arg = format!("name={rule_name}");
    let prog_arg = format!("program={program}");
    vec![
        vec![
            "advfirewall".into(), "firewall".into(), "delete".into(), "rule".into(),
            name_arg.clone(),
        ],
        vec![
            "advfirewall".into(), "firewall".into(), "add".into(), "rule".into(),
            name_arg.clone(), "dir=in".into(), "action=allow".into(),
            prog_arg.clone(), "enable=yes".into(), "profile=any".into(),
        ],
        vec![
            "advfirewall".into(), "firewall".into(), "add".into(), "rule".into(),
            name_arg, "dir=out".into(), "action=allow".into(),
            prog_arg, "enable=yes".into(), "profile=any".into(),
        ],
    ]
}

/// Program currently bound to `rule_name`, if the rule exists (unelevated —
/// `show rule` needs no admin).
fn current_rule_program(rule_name: &str) -> Option<String> {
    let out = crate::supervisor::platform::command("netsh")
        .args([
            "advfirewall", "firewall", "show", "rule",
            &format!("name={rule_name}"), "verbose",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_rule_program(&String::from_utf8_lossy(&out.stdout))
}

/// Ensure inbound+outbound allow rules exist for `program` under `rule_name`.
/// No-op when the rule already points at this exact path. Rule creation needs
/// elevation → ONE `RunAs` PowerShell shot (single UAC prompt), transcript to
/// the FEM log dir, parent blocks on the exit code. Failure is non-fatal —
/// the caller keeps starting the integration (Windows will simply prompt).
pub fn ensure_program_rules(rule_name: &str, program: &Path) -> Result<()> {
    let program_str = program.to_string_lossy().to_string();
    if let Some(existing) = current_rule_program(rule_name) {
        if existing == program_str.to_lowercase() {
            info!(rule = rule_name, "Firewall rule already matches binary path");
            return Ok(());
        }
        info!(rule = rule_name, old = %existing, new = %program_str, "Firewall rule path is stale — refreshing");
    } else {
        info!(rule = rule_name, program = %program_str, "Firewall rule missing — creating");
    }

    // PowerShell single-quoted strings escape ' by doubling it — required for
    // paths containing quotes (e.g. C:\Users\O'Brien\…).
    let ps_quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let netsh_script = reconcile_commands(rule_name, &program_str)
        .into_iter()
        .map(|argv| {
            let quoted: Vec<String> = argv
                .iter()
                .map(|a| {
                    if a.starts_with("program=") {
                        format!("{}={}", "program", ps_quote(a.trim_start_matches("program=")))
                    } else {
                        a.clone()
                    }
                })
                .collect();
            format!("netsh {}", quoted.join(" "))
        })
        .collect::<Vec<_>>()
        .join("; ");

    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("FryEdgeMiner")
        .join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let transcript = log_dir.join(format!(
        "firewall-{}.log",
        chrono::Utc::now().timestamp()
    ));

    // Inner elevated command; Start-Process -Wait keeps the outer (unelevated)
    // powershell blocking until the elevated one exits.
    let inner = format!(
        "Start-Transcript -Path {} | Out-Null; {}; Stop-Transcript | Out-Null",
        ps_quote(&transcript.display().to_string()),
        netsh_script
    );
    let outer = format!(
        "$p = Start-Process -FilePath powershell -ArgumentList '-NoProfile','-Command',\"{}\" -Verb RunAs -Wait -PassThru; exit $p.ExitCode",
        inner.replace('"', "`\"")
    );

    let out = crate::supervisor::platform::command("powershell")
        .args(["-NoProfile", "-Command", &outer])
        .output()?;
    if out.status.success() {
        info!(rule = rule_name, transcript = %transcript.display(), "Firewall rules reconciled");
        Ok(())
    } else {
        // 1223 = UAC declined.
        warn!(
            rule = rule_name,
            code = out.status.code(),
            "Firewall rule creation failed (UAC declined or netsh error) — continuing without rules"
        );
        anyhow::bail!(
            "firewall rule creation failed (exit {:?})",
            out.status.code()
        )
    }
}

/// Delete the rules (elevated, warn-only). Used by force-clean/uninstall.
pub fn delete_rules(rule_name: &str) {
    if current_rule_program(rule_name).is_none() {
        return;
    }
    let outer = format!(
        "$p = Start-Process -FilePath netsh -ArgumentList 'advfirewall','firewall','delete','rule','name={rule_name}' -Verb RunAs -Wait -PassThru; exit $p.ExitCode"
    );
    match crate::supervisor::platform::command("powershell")
        .args(["-NoProfile", "-Command", &outer])
        .output()
    {
        Ok(o) if o.status.success() => info!(rule = rule_name, "Firewall rules deleted"),
        Ok(o) => warn!(rule = rule_name, code = o.status.code(), "Firewall rule delete failed"),
        Err(e) => warn!(rule = rule_name, error = %e, "Firewall rule delete could not run"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_program_line_from_netsh_verbose_output() {
        let output = "\r\nRule Name:      FEM-OlostepBrowser\r\n----------------------------------------------------------------------\r\nEnabled:        Yes\r\nDirection:      In\r\nProfiles:       Domain,Private,Public\r\nGrouping:       \r\nLocalIP:        Any\r\nRemoteIP:       Any\r\nProtocol:       Any\r\nEdge traversal: No\r\nProgram:        C:\\Users\\u\\AppData\\Local\\Olostep-Browser\\app-1.2.3\\OlostepBrowser.exe\r\nInterfaceTypes: Any\r\nSecurity:       NotRequired\r\nAction:         Allow\r\n";
        assert_eq!(
            parse_rule_program(output).as_deref(),
            Some("c:\\users\\u\\appdata\\local\\olostep-browser\\app-1.2.3\\olostepbrowser.exe")
        );
    }

    #[test]
    fn missing_program_line_is_none() {
        assert_eq!(parse_rule_program("No rules match the specified criteria.\r\n"), None);
    }

    #[test]
    fn reconcile_builds_delete_then_in_and_out_allow_rules() {
        let cmds = reconcile_commands("FEM-OlostepBrowser", r"C:\x\OlostepBrowser.exe");
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[0][2], "delete");
        assert!(cmds[1].contains(&"dir=in".to_string()));
        assert!(cmds[2].contains(&"dir=out".to_string()));
        for add in &cmds[1..] {
            assert!(add.contains(&"action=allow".to_string()));
            assert!(add.contains(&r"program=C:\x\OlostepBrowser.exe".to_string()));
            assert!(add.contains(&"name=FEM-OlostepBrowser".to_string()));
        }
    }
}
