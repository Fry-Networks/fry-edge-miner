//! Windows Firewall rule management for partner binaries (v0.4.8).
//!
//! Squirrel-updated partner apps (OlostepBrowser) change their install path on
//! every self-update (`app-X.Y.Z`), so Windows re-prompts the firewall dialog
//! at every launch. FEM pre-creates allow rules for the exact binary path at
//! integration start, refreshing them when the path changes.
use crate::supervisor::platform::BoundedOutput;

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
            "advfirewall".into(),
            "firewall".into(),
            "delete".into(),
            "rule".into(),
            name_arg.clone(),
        ],
        vec![
            "advfirewall".into(),
            "firewall".into(),
            "add".into(),
            "rule".into(),
            name_arg.clone(),
            "dir=in".into(),
            "action=allow".into(),
            prog_arg.clone(),
            "enable=yes".into(),
            "profile=any".into(),
        ],
        vec![
            "advfirewall".into(),
            "firewall".into(),
            "add".into(),
            "rule".into(),
            name_arg,
            "dir=out".into(),
            "action=allow".into(),
            prog_arg,
            "enable=yes".into(),
            "profile=any".into(),
        ],
    ]
}

/// Program currently bound to `rule_name`, if the rule exists (unelevated —
/// `show rule` needs no admin). `pub(crate)` so callers outside this module
/// (v0.4.29 canary fix: `security_setup::run_hardening_elevated`) can verify
/// a rule genuinely landed rather than trusting an elevated script's exit
/// code alone.
pub(crate) fn current_rule_program(rule_name: &str) -> Option<String> {
    let out = crate::supervisor::platform::command("netsh")
        .args([
            "advfirewall",
            "firewall",
            "show",
            "rule",
            &format!("name={rule_name}"),
            "verbose",
        ])
        .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
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
pub fn ensure_program_rules(
    rule_name: &str,
    program: &Path,
    purpose: &'static str,
    trigger: crate::elevation_gate::ElevationTrigger,
) -> Result<()> {
    let program_str = program.to_string_lossy().to_string();
    if let Some(existing) = current_rule_program(rule_name) {
        if existing == program_str.to_lowercase() {
            info!(
                rule = rule_name,
                "Firewall rule already matches binary path"
            );
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
                        format!(
                            "{}={}",
                            "program",
                            ps_quote(a.trim_start_matches("program="))
                        )
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
    let transcript = log_dir.join(format!("firewall-{}.log", chrono::Utc::now().timestamp()));

    // Inner elevated command; Start-Process -Wait keeps the outer (unelevated)
    // powershell blocking until the elevated one exits.
    let inner = format!(
        "Start-Transcript -Path {} | Out-Null; {}; Stop-Transcript | Out-Null",
        ps_quote(&transcript.display().to_string()),
        netsh_script
    );
    let outer = format!(
        "$ErrorActionPreference = 'Stop'; try {{ $p = Start-Process -FilePath powershell -ArgumentList '-NoProfile','-WindowStyle','Hidden','-Command',\"{}\" -Verb RunAs -WindowStyle Hidden -Wait -PassThru; if ($null -eq $p) {{ exit 3 }}; exit $p.ExitCode }} catch {{ exit 2 }}",
        inner.replace('"', "`\"")
    );

    // B3: the prompt goes through the elevation gate, so FEM never raises UAC
    // on its own — an Automatic trigger is refused before the shot is fired,
    // and a UserClick gets exactly one attempt per target per process run. The
    // attempt key carries the target so a genuinely NEW binary path re-arms
    // that one attempt instead of being silently suppressed.
    let attempt_key = format!("{rule_name}|{}", program_str.to_lowercase());
    let outcome = crate::elevation_gate::run_elevated(purpose, &attempt_key, trigger, || {
        // UAC_ANSWER_TIMEOUT, not PROBE_TIMEOUT: this blocks on a HUMAN
        // answering a consent dialog, which the 20 s probe budget cannot cover.
        let out = crate::supervisor::platform::command("powershell")
            .args(["-NoProfile", "-Command", &outer])
            .output_bounded(crate::supervisor::platform::UAC_ANSWER_TIMEOUT)?;
        if out.status.success() {
            info!(rule = rule_name, transcript = %transcript.display(), "Firewall rules reconciled");
            return Ok(());
        }
        // 1223 = UAC declined. Reported as PermissionDenied so the gate
        // recognises a decline and shows the approval message rather than a
        // raw exit code.
        if crate::elevation_gate::is_declined(out.status.code(), None) {
            return Err(anyhow::Error::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "firewall rule creation declined (exit {:?})",
                    out.status.code()
                ),
            )));
        }
        anyhow::bail!(
            "firewall rule creation failed (exit {:?})",
            out.status.code()
        )
    });

    match outcome {
        Ok(()) => Ok(()),
        Err(skipped) => {
            warn!(
                rule = rule_name,
                reason = %skipped,
                "Firewall rules not reconciled — continuing without them"
            );
            anyhow::bail!("{skipped}")
        }
    }
}

/// Delete the rules (elevated, warn-only). Used by force-clean/uninstall.
pub fn delete_rules(
    rule_name: &str,
    purpose: &'static str,
    trigger: crate::elevation_gate::ElevationTrigger,
) {
    if current_rule_program(rule_name).is_none() {
        return;
    }
    let outer = format!(
        "$ErrorActionPreference = 'Stop'; try {{ $p = Start-Process -FilePath netsh -ArgumentList 'advfirewall','firewall','delete','rule','name={rule_name}' -Verb RunAs -WindowStyle Hidden -Wait -PassThru; if ($null -eq $p) {{ exit 3 }}; exit $p.ExitCode }} catch {{ exit 2 }}"
    );
    let attempt_key = format!("delete|{rule_name}");
    let outcome = crate::elevation_gate::run_elevated(purpose, &attempt_key, trigger, || {
        // Same human-answer budget as the create path.
        let o = crate::supervisor::platform::command("powershell")
            .args(["-NoProfile", "-Command", &outer])
            .output_bounded(crate::supervisor::platform::UAC_ANSWER_TIMEOUT)?;
        if o.status.success() {
            return Ok(());
        }
        anyhow::bail!("firewall rule delete failed (exit {:?})", o.status.code())
    });
    match outcome {
        Ok(()) => info!(rule = rule_name, "Firewall rules deleted"),
        Err(skipped) => warn!(rule = rule_name, reason = %skipped, "Firewall rule delete skipped"),
    }
}

/// D-C5-2: what Windows Firewall says about frynode's inbound allow rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuleVerdict {
    /// An enabled inbound allow rule for this exact program covers every
    /// active network profile.
    Admitted,
    Missing,
    Disabled,
    /// Rules of this name exist, but none is an inbound allow rule.
    NotInboundAllow,
    WrongProgram,
    ProfileNotCovered,
    /// The listing could not be produced, or was cut short.
    Unreadable,
}

/// D-C5-2: the PowerShell that lists every rule named `rule_name` and the
/// active network categories, one fact per line, then `END`. It prints enum
/// NAMES (Inbound, True, Allow, Any, Public), which Windows does not translate;
/// netsh's labels and values are localized, so a netsh parse would never admit
/// the rule on a non-English Windows. Any error exits 3 without `END`.
pub(crate) fn rule_listing_script(rule_name: &str) -> String {
    let name = format!("'{}'", rule_name.replace('\'', "''"));
    format!(
        "$ErrorActionPreference = 'Stop'; try {{ \
         [Console]::OutputEncoding = [Text.Encoding]::UTF8; \
         Get-NetFirewallRule -DisplayName {name} -ErrorAction SilentlyContinue | ForEach-Object {{ \
         $f = $_ | Get-NetFirewallApplicationFilter; \
         'RULE|' + $_.Direction + '|' + $_.Enabled + '|' + $_.Action + '|' + $_.Profile + '|' + $f.Program }}; \
         Get-NetConnectionProfile -ErrorAction SilentlyContinue | ForEach-Object {{ 'NET|' + $_.NetworkCategory }}; \
         'END' }} catch {{ exit 3 }}"
    )
}

/// D-C5-2: the listing `rule_verdict` reads. Unelevated — reading rules needs
/// no admin — and bounded by PROBE_TIMEOUT. `None` when PowerShell failed or
/// timed out.
pub(crate) fn rule_listing(rule_name: &str) -> Option<String> {
    let out = crate::supervisor::platform::command("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &rule_listing_script(rule_name),
        ])
        .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// PURE (D-C5-2): does the listing hold an enabled inbound allow rule for
/// `program` that covers every active network profile? Fails closed: a missing
/// or cut-short listing is `Unreadable`, never `Admitted`.
pub(crate) fn rule_verdict(listing: Option<&str>, program: &str) -> RuleVerdict {
    let Some(listing) = listing else {
        return RuleVerdict::Unreadable;
    };
    let lines: Vec<&str> = listing.lines().map(str::trim).collect();
    if !lines.contains(&"END") {
        return RuleVerdict::Unreadable;
    }
    let mut active: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("NET|"))
        .map(|c| match c.trim() {
            "DomainAuthenticated" => "Domain",
            other => other,
        })
        .collect();
    // Windows puts a network it cannot identify on the Public profile.
    if active.is_empty() {
        active.push("Public");
    }
    let wanted = normalize_program(program);
    // Report the failure closest to a usable rule.
    let rank = |v: RuleVerdict| match v {
        RuleVerdict::Missing => 0,
        RuleVerdict::NotInboundAllow => 1,
        RuleVerdict::WrongProgram => 2,
        RuleVerdict::Disabled => 3,
        _ => 4,
    };
    let mut closest = RuleVerdict::Missing;
    for rule in lines.iter().filter_map(|l| l.strip_prefix("RULE|")) {
        let f: Vec<&str> = rule.splitn(5, '|').map(str::trim).collect();
        let [direction, enabled, action, profile, rule_program] = f[..] else {
            continue;
        };
        let verdict = if !direction.eq_ignore_ascii_case("Inbound")
            || !action.eq_ignore_ascii_case("Allow")
        {
            RuleVerdict::NotInboundAllow
        } else if !enabled.eq_ignore_ascii_case("True") {
            RuleVerdict::Disabled
        } else if normalize_program(rule_program) != wanted {
            RuleVerdict::WrongProgram
        } else if !profile_covers(profile, &active) {
            RuleVerdict::ProfileNotCovered
        } else {
            return RuleVerdict::Admitted;
        };
        if rank(verdict) > rank(closest) {
            closest = verdict;
        }
    }
    closest
}

/// `Any`, or a list such as `Domain, Private` naming every active profile.
fn profile_covers(profile: &str, active: &[&str]) -> bool {
    if profile.eq_ignore_ascii_case("Any") {
        return true;
    }
    let covered: Vec<&str> = profile.split(',').map(str::trim).collect();
    active
        .iter()
        .all(|a| covered.iter().any(|c| c.eq_ignore_ascii_case(a)))
}

/// PURE (D-C5-2): a program path in the one spelling rule and binary are
/// compared in.
pub(crate) fn normalize_program(program: &str) -> String {
    normalize_program_with(program, |name| std::env::var(name).ok())
}

/// `normalize_program` with the environment injected: surrounding quotes and
/// spaces dropped, `%VAR%` expanded, `/` read as `\`, the `\\?\` prefix
/// dropped, and case folded, as Windows paths compare.
fn normalize_program_with(program: &str, env: impl Fn(&str) -> Option<String>) -> String {
    let mut rest = program.trim().trim_matches('"');
    let mut expanded = String::new();
    while let Some(start) = rest.find('%') {
        let Some(len) = rest[start + 1..].find('%') else {
            break;
        };
        let name = &rest[start + 1..start + 1 + len];
        expanded.push_str(&rest[..start]);
        match env(name) {
            Some(value) => expanded.push_str(&value),
            None => expanded.push_str(&rest[start..start + len + 2]),
        }
        rest = &rest[start + len + 2..];
    }
    expanded.push_str(rest);
    let slashed = expanded.replace('/', "\\");
    slashed
        .strip_prefix("\\\\?\\")
        .unwrap_or(&slashed)
        .to_lowercase()
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
        assert_eq!(
            parse_rule_program("No rules match the specified criteria.\r\n"),
            None
        );
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

/// Continuation #5, D-C5-2: the rule predicate behind the fryDVPN gate.
#[cfg(test)]
#[path = "fryvpn_c5_rule_predicate_tests.rs"]
mod fryvpn_c5_rule_predicate_tests;
