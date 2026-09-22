//! B23 defect 4: the eleven rules `scrub_line` had missed every shape the
//! Done-when's remaining classes actually take.
//!
//! One test per class, plus the two guards that are what stop this degenerating
//! into a redactor that eats the log: a diagnostic line with no secret must come
//! back byte-identical, and scrubbing twice must change nothing (the debug
//! bundle scrubs a second time on the way into the zip).
//!
//! The bare-identity rules are exercised through their PURE form
//! (`identity_rules` + `redact_literals`) rather than by seeding the
//! process-global one: `cargo test` runs in parallel, and a test that mutates a
//! process-wide `OnceLock` would make its neighbours depend on scheduling.

use super::scrubber::{identity_rules, redact_literals, scrub_line};

/// A 58-char base32 Algorand address, the class the Done-when names.
const ADDRESS: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

#[test]
fn a_device_name_field_is_redacted() {
    // The exact shape pawns.rs emits.
    let line = "Starting Pawns.app agent device_name=GEORGE-RIG-01 device_id=fem-george-rig-01";
    let out = scrub_line(line);
    assert!(!out.contains("GEORGE-RIG-01"), "{out}");
    assert!(!out.contains("fem-george-rig-01"), "{out}");
    assert!(
        out.contains("device_name="),
        "the field must survive: {out}"
    );
}

#[test]
fn a_wireguard_key_is_redacted() {
    let line = "WG public key: kzisBQWgJeKo5uQN5X457OlzMVirwtIN4gTfe9uy1QA=";
    let out = scrub_line(line);
    assert!(
        !out.contains("kzisBQWgJeKo5uQN5X457OlzMVirwtIN4gTfe9uy1QA="),
        "{out}"
    );
    assert!(out.contains("[WGKEY]"), "{out}");
}

#[test]
fn a_password_passed_as_a_cli_flag_is_redacted() {
    // The literal argv shape pawns.rs builds for `docker run`.
    let line = "docker run --rm -password=hunter2-not-a-real-secret -email=x@y.z";
    let out = scrub_line(line);
    assert!(!out.contains("hunter2-not-a-real-secret"), "{out}");
    assert!(
        out.contains("-email=x@y.z"),
        "an unrelated flag must survive: {out}"
    );
}

#[test]
fn an_api_key_passed_as_a_space_separated_flag_is_redacted() {
    let out = scrub_line("invoking partner with --api-key ABCDEF123456XYZ");
    assert!(!out.contains("ABCDEF123456XYZ"), "{out}");
}

#[test]
fn a_json_node_token_is_redacted() {
    // The file format iagon.rs tells the user to create.
    let out = scrub_line(r#"read config {"node_token": "iagon-key-value"}"#);
    assert!(!out.contains("iagon-key-value"), "{out}");
    assert!(out.contains("node_token"), "the key must survive: {out}");
}

#[test]
fn a_node_mnemonic_env_assignment_is_redacted() {
    let out = scrub_line("env NODE_MNEMONIC=abandon-abandon-ability starting frynode");
    assert!(!out.contains("abandon-abandon-ability"), "{out}");
}

#[test]
fn a_comma_separated_mixed_case_mnemonic_is_redacted() {
    // The original rule needs exactly 25 lowercase space-separated words and
    // has no case-insensitivity, so this form walked straight through it.
    let words: Vec<&str> = std::iter::repeat("Abandon")
        .take(23)
        .chain(["Ability"])
        .collect();
    let line = format!("recovery: {}", words.join(", "));
    let out = scrub_line(&line);
    assert!(out.contains("[MNEMONIC]"), "{out}");
    assert!(!out.contains("Ability"), "{out}");
}

#[test]
fn a_bare_computer_name_and_username_are_redacted() {
    let rules = identity_rules(Some("GEORGE-RIG-01"), Some("georgep"));
    assert_eq!(rules.len(), 2, "both values must produce a rule");

    let out = redact_literals(
        "titan-edge failed on GEORGE-RIG-01 for user georgep",
        &rules,
    );
    assert!(!out.contains("GEORGE-RIG-01"), "{out}");
    assert!(!out.contains("georgep"), "{out}");
    assert!(out.contains("<host>") && out.contains("<user>"), "{out}");
}

/// Two-character values are not identifying enough to be worth the
/// over-redaction they would cause across every log line.
#[test]
fn a_too_short_identity_value_is_not_turned_into_a_rule() {
    assert!(identity_rules(Some("PC"), Some("")).is_empty());
}

/// The frynode line the Done-when is really about. The Algorand rule already
/// existed; this pins that it still fires through the new pipeline.
#[test]
fn the_frynode_startup_address_line_is_reduced_to_first_and_last_four() {
    let out = scrub_line(&format!("Node address: {ADDRESS}"));
    assert!(!out.contains(ADDRESS), "{out}");
    assert!(out.contains("AAAA…AAAA"), "{out}");
    assert!(
        out.contains("Node address:"),
        "the diagnostic must survive: {out}"
    );
}

/// GUARD 1. A redactor that eats everything would pass every one-way
/// assertion above. These must come back byte-identical.
#[test]
fn a_diagnostic_line_with_no_secret_survives_intact() {
    for line in [
        "Integration titan restart 3/5 after exit code 740",
        r"Install dir: C:\Program Files\Fry Edge Miner",
        "Spawning process integration=fryvpn",
        "the secret is safe and there is no assignment here",
        "Docker unavailable — the daemon did not answer within 20s",
    ] {
        assert_eq!(scrub_line(line), line, "over-redacted: {line}");
    }
}

/// GUARD 2. The bundle export scrubs a second time on the way into the zip, so
/// a rule that rewrites its own output would change the file between passes.
#[test]
fn scrub_line_is_still_idempotent_with_the_new_rules() {
    for line in [
        "Starting Pawns.app agent device_name=GEORGE-RIG-01 device_id=fem-george-rig-01",
        "WG public key: kzisBQWgJeKo5uQN5X457OlzMVirwtIN4gTfe9uy1QA=",
        "docker run --rm -password=hunter2-not-a-real-secret -email=x@y.z",
        r#"read config {"node_token": "iagon-key-value"}"#,
        r#"api_key = "secret-key-12345""#,
        "Authorization: Bearer sk-test123abc456def789",
        r"Config path: D:\Users\alice\AppData\Roaming",
        "Integration titan restart 3/5 after exit code 740",
    ] {
        let once = scrub_line(line);
        let twice = scrub_line(&once);
        assert_eq!(once, twice, "not idempotent for: {line}");
    }
}

/// The existing markers must not be rewritten by the new rules — several
/// existing tests assert on them verbatim.
#[test]
fn an_existing_redaction_marker_is_never_rewritten() {
    assert_eq!(scrub_line("api_key=[REDACTED]"), "api_key=[REDACTED]");
    assert_eq!(scrub_line("token=[REDACTED]"), "token=[REDACTED]");
    assert_eq!(scrub_line("hostname=<host>"), "hostname=<host>");
    assert_eq!(
        scrub_line("device_name=<redacted>"),
        "device_name=<redacted>"
    );
}
