//! U6 RED->GREEN tests for FEM-DISCORD-HARDENING-LOOP.

use std::time::{Duration, Instant};

const FIREWALL_RS: &str = include_str!("firewall.rs");
const SECURITY_SETUP_RS: &str = include_str!("../security_setup.rs");
const HARDENING_CMD_RS: &str = include_str!("../commands/hardening.rs");

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn clear_blocked_does_not_block_behind_a_held_gate() {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let _ = crate::elevation_gate::run_elevated(
            "u6-gate-holder-test",
            "hold-1",
            crate::elevation_gate::ElevationTrigger::UserClick,
            || {
                let _ = tx.send(());
                std::thread::sleep(Duration::from_millis(350));
                Ok(())
            },
        );
    });
    rx.recv_timeout(Duration::from_secs(2))
        .expect("holder closure must start");
    std::thread::sleep(Duration::from_millis(20));

    let t0 = Instant::now();
    crate::elevation_gate::clear_blocked("u6-other-purpose-nonblocking");
    let elapsed = t0.elapsed();

    handle.join().expect("holder thread panicked");
    assert!(
        elapsed < Duration::from_millis(100),
        "clear_blocked blocked for {elapsed:?} behind a held elevation gate"
    );
}

#[test]
fn firewall_and_security_setup_provide_native_netsh_fallback_verifier() {
    let fw_code = code_only(FIREWALL_RS);
    let sec_code = code_only(SECURITY_SETUP_RS);
    assert!(
        fw_code.contains("fn netsh_rule_verdicts("),
        "firewall.rs must expose a native netsh advfirewall rule verifier for PowerShell 0xe0434352 fallback"
    );
    assert!(
        fw_code.contains("fn verify_program_rules("),
        "firewall.rs must expose verify_program_rules combining PowerShell CIM + netsh fallback"
    );
    assert!(
        sec_code.contains("verify_program_rules("),
        "security_setup.rs must use verify_program_rules so 0xe0434352 PowerShell crashes do not loop"
    );
}

#[test]
fn retry_hardening_clears_fryvpn_parked_rule_and_checks_inflight_gate() {
    let cmd_code = code_only(HARDENING_CMD_RS);
    assert!(
        cmd_code.contains("clear_rule_park_if_admitted"),
        "commands/hardening.rs must clear fryvpn::PARKED_RULE_REASON after retry_hardening succeeds"
    );
    assert!(
        cmd_code.contains("is_elevation_in_flight"),
        "commands/hardening.rs must do a non-blocking gate() check before blocking on another elevation"
    );
}

#[test]
fn netsh_rule_verdicts_parses_valid_disabled_wrong_program_and_user_renamed_rules() {
    use super::firewall::{netsh_rule_verdicts, RuleVerdict};

    let prog = r"C:\Users\u\AppData\Local\Fry Edge Miner\resources\frynode.exe";
    let valid = format!(
        "\r\nRule Name: FEM-FryNode\r\nEnabled: Yes\r\nDirection: In\r\nProfiles: Domain,Private,Public\r\nProgram: {prog}\r\nAction: Allow\r\n\r\nRule Name: FEM-FryNode\r\nEnabled: Yes\r\nDirection: Out\r\nProfiles: Domain,Private,Public\r\nProgram: {prog}\r\nAction: Allow\r\n"
    );
    assert_eq!(
        netsh_rule_verdicts(&valid, prog),
        (RuleVerdict::Admitted, RuleVerdict::Admitted)
    );

    // User manually created allow rules under a different display name (dilijasi report).
    let custom_name = format!(
        "Rule Name: My Custom Fry Rule\nEnabled: Yes\nDirection: In\nProfiles: Any\nProgram: {prog}\nAction: Allow\n\nRule Name: My Custom Fry Rule Out\nEnabled: Yes\nDirection: Out\nProfiles: Any\nProgram: {prog}\nAction: Allow\n"
    );
    assert_eq!(
        netsh_rule_verdicts(&custom_name, prog),
        (RuleVerdict::Admitted, RuleVerdict::Admitted)
    );

    let disabled = format!(
        "Rule Name: FEM-FryNode\nEnabled: No\nDirection: In\nProfiles: Any\nProgram: {prog}\nAction: Allow\n"
    );
    assert_eq!(
        netsh_rule_verdicts(&disabled, prog),
        (RuleVerdict::Disabled, RuleVerdict::NotInboundAllow)
    );

    assert_eq!(
        netsh_rule_verdicts("No rules match the specified criteria.\r\n", prog),
        (RuleVerdict::Missing, RuleVerdict::Missing)
    );
}
