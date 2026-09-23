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

use super::scrubber::{identity_rules, redact_literals, scrub_line, scrub_partner_line};

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
    let words: Vec<&str> = std::iter::repeat_n("Abandon", 23)
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

// ---------------------------------------------------------------------------
// B23 defect 2, the write-time layer. These exist because always-on scrubbing
// of a partner's own stdout broke a PRE-EXISTING invariant test in RC build
// 35800402767: `bug9_working_dir_tests` spawns a child, reads the working
// directory it reports out of that very log, and canonicalizes it. A
// substituted username cannot survive `canonicalize`, which requires the path
// to exist. The test was right and the scrubbing was in the wrong layer.
// ---------------------------------------------------------------------------

/// The exact line from the RC failure. It must come back BYTE-IDENTICAL.
#[test]
fn a_partner_reporting_its_working_directory_is_left_byte_identical() {
    let reported = r"C:\Users\runneradmin\AppData\Local\Temp\fem-bug9-5772\partner-home";
    assert_eq!(
        scrub_partner_line(reported),
        reported,
        "a partner's own path must reach the log unchanged — FEM and its tests \
         read these back and canonicalize them, and `canonicalize` requires the \
         path to exist"
    );
}

/// …and the same holds for every partner path shape in the tree.
#[test]
fn no_partner_path_is_rewritten_on_the_way_to_disk() {
    for line in [
        r"C:\Users\georgep\AppData\Roaming\FryEdgeMiner\partners\titan",
        r"D:\fry_storage\FryEdgeMiner\partners\titan\titan-edge.exe",
        "storage dir /home/fry/.local/share/FryEdgeMiner/partners",
        r"titan-edge_v0.1.20_246b9dd_widnows_amd64",
        "starting API server on :8088",
    ] {
        assert_eq!(scrub_partner_line(line), line, "path was rewritten: {line}");
    }
}

/// The reason the write-time layer exists at all: what the shipped frynode.exe
/// prints on startup, confirmed in its string table.
#[test]
fn what_frynode_prints_on_startup_never_reaches_the_log_raw() {
    let address = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let out = scrub_partner_line(&format!("Node address: {address}"));
    assert!(
        !out.contains(address),
        "wallet address reached the log: {out}"
    );
    assert!(out.contains("Node address:"), "diagnostic lost: {out}");

    let wg = "kzisBQWgJeKo5uQN5X457OlzMVirwtIN4gTfe9uy1QA=";
    let out = scrub_partner_line(&format!("WG public key: {wg}"));
    assert!(!out.contains(wg), "WG key reached the log: {out}");
}

/// Every other irreversible class the Done-when names is covered too.
#[test]
fn a_partner_leaking_a_token_or_mnemonic_is_still_scrubbed() {
    for (line, secret) in [
        (
            r#"read config {"node_token": "iagon-key-value"}"#,
            "iagon-key-value",
        ),
        (
            "docker run --rm -password=hunter2-not-a-real-secret",
            "hunter2-not-a-real-secret",
        ),
        (
            "NODE_MNEMONIC=abandon-abandon-ability",
            "abandon-abandon-ability",
        ),
        ("Authorization: Bearer sk-test123abc456def789", "sk-test"),
    ] {
        let out = scrub_partner_line(line);
        assert!(!out.contains(secret), "{secret} survived: {out}");
    }
}

/// THE CLAUSE THAT STILL HOLDS. The artifact users actually post publicly is
/// the exported bundle, and that is scrubbed with the FULL rule set at
/// collection time — including the username, which write-time deliberately
/// leaves alone.
#[test]
fn the_full_scrubber_still_removes_the_username_for_the_exported_bundle() {
    let line = r"C:\Users\georgep\AppData\Roaming\FryEdgeMiner\partners\titan";
    let bundled = scrub_line(line);
    assert!(
        !bundled.contains("georgep"),
        "the bundle path must still strip the username: {bundled}"
    );
    assert!(bundled.contains("<user>"), "{bundled}");
    // And the two layers genuinely differ — if they ever converge, the write-time
    // layer has started rewriting paths again.
    assert_ne!(
        scrub_partner_line(line),
        bundled,
        "write-time and collection-time scrubbing must NOT be the same rule set"
    );
}

/// Wiring: the partner pipe must use the narrow rule set and the bundle the
/// full one. Needles assembled at runtime so this cannot match its own text.
#[test]
fn each_sink_uses_the_rule_set_it_is_supposed_to() {
    let partner_fn = format!("scrub{}partner{}line(", "_", "_");

    let process_rs = include_str!("../supervisor/process.rs");
    assert!(
        process_rs.contains(&partner_fn),
        "the partner pipe must scrub with the narrow, path-preserving rule set"
    );

    // Matched on the collection MAP itself, not on the file. My first version
    // asserted `debug_rs.contains("scrub_line(") && !debug_rs.contains("scrub_partner_line(")`
    // over the whole file, and mutation testing showed that was vacuous:
    // debug.rs's own idempotency test mentions `scrub_line(`, so swapping the
    // real collection call to the narrow rule set left it green.
    let debug_rs = include_str!("../commands/debug.rs");
    let full_map = format!(".map(scrubber::scrub{}line)", "_");
    let narrow_map = format!(".map(scrubber::scrub{}partner{}line)", "_", "_");
    assert!(
        debug_rs.contains(&full_map),
        "the bundle must keep the FULL rule set at collection time — it is the \
         artifact that leaves the machine, and the username clause is only \
         satisfiable there"
    );
    assert!(
        !debug_rs.contains(&narrow_map),
        "the bundle must NOT be collected with the narrow write-time rule set"
    );
}

// ---------------------------------------------------------------------------
// G4 findings 17 and 19: the corpus had no MystNodes case, which is exactly
// where a shipped secret lives. mysterium.rs starts sdk_client with
// `--user.token=<device token>` in argv and carries a comment asserting the
// line is scrubbed before it is surfaced. It was not: SECRET_NAMES' prefix
// class could not span the `.` in `user.token`, so the flag arm died right
// after `--`.
// ---------------------------------------------------------------------------

/// The exact shape mysterium.rs builds, in both sinks.
#[test]
fn a_mystnodes_user_token_is_redacted_in_both_sinks() {
    for line in [
        "2026-09-22T10:00:00Z ERR sdk_client --user.token=mystSECRET123",
        "FTL failed to start args=[--user.token=mystSECRET123 --log.level=info]",
        "launching sdk_client --user.token=mystSECRET123",
    ] {
        let full = scrub_line(line);
        assert!(
            !full.contains("mystSECRET123"),
            "the device token reached the bundle: {full}"
        );
        let partner = scrub_partner_line(line);
        assert!(
            !partner.contains("mystSECRET123"),
            "the device token reached the partner log: {partner}"
        );
    }
}

/// A hex-shaped token must not depend on `redact_serial` to be caught, because
/// `scrub_partner_line` deliberately omits that rule (it rewrites paths). The
/// NAME is what makes it a secret.
#[test]
fn a_hex_shaped_token_is_caught_by_its_name_not_its_shape() {
    let line = "ERR sdk_client --user.token=deadbeefdeadbeef";
    let partner = scrub_partner_line(line);
    assert!(
        !partner.contains("deadbeefdeadbeef"),
        "must be caught by the name, since redact_serial is not in this rule \
         set: {partner}"
    );
}

/// The colon form. Only `token:` and `api-key:` had a colon rule.
#[test]
fn the_colon_form_of_a_named_secret_is_redacted() {
    for (line, secret) in [
        ("config password: hunter2-not-real", "hunter2-not-real"),
        ("loaded secret: abc123xyz", "abc123xyz"),
        ("private_key: MIIEvQIBADAN", "MIIEvQIBADAN"),
        ("sdk_client user.token: mystSECRET456", "mystSECRET456"),
    ] {
        let out = scrub_line(line);
        assert!(!out.contains(secret), "{secret} survived: {out}");
        let partner = scrub_partner_line(line);
        assert!(!partner.contains(secret), "{secret} survived: {partner}");
    }
}

/// …without eating a Windows path or a URL, which is why the colon arm requires
/// whitespace after the colon.
#[test]
fn the_colon_arm_does_not_eat_paths_or_urls() {
    for line in [
        r"Install dir: C:\Program Files\Fry Edge Miner",
        r"C:\Users\georgep\AppData\Roaming\FryEdgeMiner\partners\titan",
        "endpoint https://hardwareapi.frynetworks.com/v1/installations",
        "starting API server on :8088",
    ] {
        assert_eq!(
            scrub_partner_line(line),
            line,
            "the write-time set must leave this byte-identical: {line}"
        );
    }
}
