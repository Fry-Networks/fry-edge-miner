//! Continuation #5, D-C5-2: the pure predicate behind the fryDVPN rule gate.
//! frynode is spawned only when an enabled inbound allow rule named
//! FEM-FryNode points at the installed frynode.exe and covers every active
//! network profile. The listing is what `rule_listing_script` prints on
//! Windows: one `RULE|` line per rule of that name, one `NET|` line per active
//! network, then `END`.

use super::*;

const FRYNODE: &str = r"C:\Users\u\AppData\Local\Fry Edge Miner\resources\frynode.exe";

fn listing(lines: &[&str]) -> String {
    let mut text = lines.join("\r\n");
    text.push_str("\r\nEND\r\n");
    text
}

fn verdict(lines: &[&str]) -> RuleVerdict {
    rule_verdict(Some(&listing(lines)), FRYNODE)
}

/// v0.4.33 (tag 2469e19, src-tauri/src/integrations/firewall.rs:30-65) creates
/// the rule with exactly this argv — the inbound half of it is what 0.4.33
/// upgraders already hold:
///   advfirewall firewall add rule name=FEM-FryNode dir=in action=allow
///   program=<frynode.exe> enable=yes profile=any
/// Windows lists such a rule (and its outbound twin) as below.
#[test]
fn the_rule_v0_4_33_creates_is_admitted() {
    let created = reconcile_commands("FEM-FryNode", FRYNODE);
    assert_eq!(
        created[1],
        [
            "advfirewall",
            "firewall",
            "add",
            "rule",
            "name=FEM-FryNode",
            "dir=in",
            "action=allow",
            &format!("program={FRYNODE}"),
            "enable=yes",
            "profile=any",
        ],
        "the inbound rule is created exactly as v0.4.33 created it"
    );
    assert_eq!(
        verdict(&[
            &format!("RULE|Inbound|True|Allow|Any|{FRYNODE}"),
            &format!("RULE|Outbound|True|Allow|Any|{FRYNODE}"),
            "NET|Public",
        ]),
        RuleVerdict::Admitted
    );
}

#[test]
fn a_missing_rule_is_not_admitted() {
    assert_eq!(verdict(&["NET|Private"]), RuleVerdict::Missing);
    assert_eq!(verdict(&[]), RuleVerdict::Missing);
}

#[test]
fn a_disabled_rule_is_not_admitted() {
    assert_eq!(
        verdict(&[
            &format!("RULE|Inbound|False|Allow|Any|{FRYNODE}"),
            "NET|Public"
        ]),
        RuleVerdict::Disabled
    );
}

#[test]
fn only_an_inbound_allow_rule_counts() {
    assert_eq!(
        verdict(&[
            &format!("RULE|Outbound|True|Allow|Any|{FRYNODE}"),
            "NET|Public"
        ]),
        RuleVerdict::NotInboundAllow,
        "an outbound rule does not let clients reach frynode"
    );
    assert_eq!(
        verdict(&[
            &format!("RULE|Inbound|True|Block|Any|{FRYNODE}"),
            "NET|Public"
        ]),
        RuleVerdict::NotInboundAllow,
        "a block rule is the opposite of what frynode needs"
    );
}

#[test]
fn a_rule_for_another_program_is_not_admitted() {
    assert_eq!(
        verdict(&[
            r"RULE|Inbound|True|Allow|Any|C:\Program Files\Other\frynode.exe",
            "NET|Public"
        ]),
        RuleVerdict::WrongProgram
    );
    assert_eq!(
        verdict(&["RULE|Inbound|True|Allow|Any|Any", "NET|Public"]),
        RuleVerdict::WrongProgram,
        "a rule for every program is not the rule FEM creates and checks"
    );
}

#[test]
fn the_rule_must_cover_every_active_network_profile() {
    let rule = |profile: &str| format!("RULE|Inbound|True|Allow|{profile}|{FRYNODE}");
    assert_eq!(
        verdict(&[&rule("Private"), "NET|Public"]),
        RuleVerdict::ProfileNotCovered
    );
    assert_eq!(
        verdict(&[&rule("Domain, Private"), "NET|Private"]),
        RuleVerdict::Admitted
    );
    assert_eq!(
        verdict(&[&rule("Domain"), "NET|DomainAuthenticated"]),
        RuleVerdict::Admitted,
        "a domain network's category is DomainAuthenticated"
    );
    assert_eq!(
        verdict(&[&rule("Private"), "NET|Private", "NET|Public"]),
        RuleVerdict::ProfileNotCovered,
        "two active networks: the rule must cover both"
    );
    assert_eq!(
        verdict(&[&rule("Private")]),
        RuleVerdict::ProfileNotCovered,
        "with no network reported, the Public profile Windows gives an unidentified \
         network is assumed"
    );
    assert_eq!(verdict(&[&rule("Any")]), RuleVerdict::Admitted);
}

#[test]
fn one_matching_rule_is_enough_among_several_of_the_same_name() {
    assert_eq!(
        verdict(&[
            &format!("RULE|Inbound|False|Allow|Any|{FRYNODE}"),
            r"RULE|Inbound|True|Allow|Any|C:\old\frynode.exe",
            &format!("RULE|Inbound|True|Allow|Any|{FRYNODE}"),
            "NET|Public",
        ]),
        RuleVerdict::Admitted
    );
}

#[test]
fn an_unreadable_listing_is_never_admitted() {
    assert_eq!(rule_verdict(None, FRYNODE), RuleVerdict::Unreadable);
    assert_eq!(
        rule_verdict(
            Some(&format!("RULE|Inbound|True|Allow|Any|{FRYNODE}\r\n")),
            FRYNODE
        ),
        RuleVerdict::Unreadable,
        "a listing cut short before END proves nothing"
    );
}

#[test]
fn program_paths_are_compared_after_normalisation() {
    let env =
        |name: &str| (name == "LOCALAPPDATA").then(|| r"C:\Users\u\AppData\Local".to_string());
    let normal = normalize_program_with(FRYNODE, env);
    for spelling in [
        r"c:\users\U\appdata\local\fry edge miner\resources\FRYNODE.EXE",
        "C:/Users/u/AppData/Local/Fry Edge Miner/resources/frynode.exe",
        r#""C:\Users\u\AppData\Local\Fry Edge Miner\resources\frynode.exe""#,
        r"%LOCALAPPDATA%\Fry Edge Miner\resources\frynode.exe",
        r"\\?\C:\Users\u\AppData\Local\Fry Edge Miner\resources\frynode.exe",
        r"  C:\Users\u\AppData\Local\Fry Edge Miner\resources\frynode.exe  ",
    ] {
        assert_eq!(normalize_program_with(spelling, env), normal, "{spelling}");
    }
    assert_ne!(
        normalize_program_with(r"C:\Users\u\AppData\Local\Other\frynode.exe", env),
        normal
    );
}

#[test]
fn the_listing_script_reads_locale_invariant_fields_and_fails_loudly() {
    let script = rule_listing_script("FEM-FryNode");
    for needed in [
        "$ErrorActionPreference = 'Stop'",
        "Get-NetFirewallRule -DisplayName 'FEM-FryNode'",
        "Get-NetFirewallApplicationFilter",
        "Get-NetConnectionProfile",
        "'RULE|'",
        "'NET|'",
        "'END'",
        "exit 3",
    ] {
        assert!(script.contains(needed), "missing {needed:?}: {script}");
    }
    assert!(
        !script.contains("RunAs") && !script.contains("netsh"),
        "the check reads the rule; it never elevates and never parses netsh's localized text"
    );
    assert!(
        rule_listing_script("O'Brien").contains("-DisplayName 'O''Brien'"),
        "the rule name is quoted for PowerShell"
    );
}

/// The gate is Windows Firewall's, and it guards the installed frynode.exe:
/// an explicit FRYNODE_BIN developer override is not what the rule is for.
#[test]
fn the_gate_applies_on_windows_to_the_installed_frynode_only() {
    use crate::integrations::fryvpn::rule_gate_applies;
    assert!(rule_gate_applies(true, false));
    assert!(!rule_gate_applies(true, true));
    assert!(!rule_gate_applies(false, false));
    assert!(!rule_gate_applies(false, true));
}
