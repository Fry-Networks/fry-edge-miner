//! BUG LOOP 2 — the miner key in every shape the product accepts or keeps.
//!
//! `redact_miner_key` matched `FEM-` + 32 HEX. The product's own contract is
//! wider: `config/miner_key.rs` accepts any 32 alphanumerics
//! (`validate_fem_key_preserve_case`), the Wizard accepts
//! `/^FEM-[A-Z0-9]{32}$/i`, and `resolve_stored_miner_key` keeps a stored key
//! that fails validation (the legacy `FEM-SHORTKEY123`). `commands/device.rs`
//! and `poc/reporter.rs` log whichever key is stored, so each of these reached
//! fem.log and the exported bundle verbatim. All keys here are synthetic.

use super::*;

/// The production shape `miner_key.rs` pins as its own synthetic fixture.
const PRODUCTION: &str = "FEM-Z21JQG5PJG5GJIX4PRXIEONBXVXQCQ2U";
const ALL_LETTERS: &str = "FEM-ABCDEFGHIJKLMNOPQRSTUVWXYZABCDEF";
const MIXED_CASE: &str = "FEM-z21jQG5pjG5gjIX4prXIeoNBxvXQcq2u";
/// A stored key the validator rejects but `resolve_stored_miner_key` keeps.
const LEGACY_SHORT: &str = "FEM-SHORTKEY123";

/// Synthetic, checksum-valid Algorand address (not a real wallet).
const ADDR: &str = "YTC4NR6IZHFMXTGNZ3H5BUOS2PKNLVWX3DM5VW643XPN7YHB4LRUS2CHMA";

/// The shapes the key is actually logged in: the two M6 bundle lines, the
/// other `commands/device.rs` emitters (`miner_key = %key`), the reporter's
/// `miner_key = key` (a quoted str), and the key inside a JSON body.
fn logged_shapes(key: &str) -> Vec<String> {
    vec![
        format!("2026-09-24T06:43:31.410820Z  INFO fry_edge_miner::commands::device: Device auto-migrated to per-device token miner_key={key}"),
        format!("2026-09-24T06:45:05.461118Z  INFO fry_edge_miner::commands::device: App version changed since last report — sending immediate heartbeat + lease action miner_key={key} from=\"0.4.29\" to=\"0.4.34\""),
        format!("2026-09-24T06:43:30.000000Z  INFO fry_edge_miner::commands::device: Attempting device token migration miner_key={key}"),
        format!("2026-09-24T06:46:00.000000Z  WARN fry_edge_miner::commands::device: Version-change lease action did not grant — the next PoC tick will retry miner_key={key}"),
        format!("2026-09-24T07:00:00.000000Z  INFO fry_edge_miner::poc::reporter: PoC submitted miner_key=\"{key}\" slot=17"),
        format!(r#"request body {{"miner_key":"{key}","device_name":"<redacted>"}}"#),
        format!(r#"request body {{"miner_key": "{key}", "slot": 17}}"#),
    ]
}

/// What must not survive: the key's body, compared case-insensitively.
fn body(key: &str) -> String {
    key.trim_start_matches("FEM-").to_ascii_lowercase()
}

fn assert_key_gone(scrub: fn(&str) -> String, path: &str) {
    for key in [PRODUCTION, ALL_LETTERS, MIXED_CASE, LEGACY_SHORT] {
        for line in logged_shapes(key) {
            let out = scrub(&line);
            assert!(
                !out.to_ascii_lowercase().contains(&body(key)),
                "{path}: the miner key {key} survived: {out}"
            );
            assert!(
                out.contains("miner_key"),
                "{path}: only the value is redacted, never the field name: {out}"
            );
            assert_eq!(scrub(&out), out, "{path}: must stay idempotent");
        }
    }
}

#[test]
fn every_accepted_or_kept_key_shape_is_redacted_from_the_bundle() {
    assert_key_gone(scrub_line, "scrub_line");
}

#[test]
fn a_partner_log_line_carrying_the_key_is_redacted_too() {
    assert_key_gone(scrub_partner_line, "scrub_partner_line");
}

/// A production-shape key is redacted even outside a `miner_key` field — the
/// request URLs FEM builds from it (`/credentials/{key}`, `/PoC/{key}/…`)
/// reach logs through reqwest errors.
#[test]
fn a_production_shape_key_in_a_request_url_is_redacted() {
    for key in [PRODUCTION, ALL_LETTERS, MIXED_CASE] {
        let line = format!(
            "WARN fry_edge_miner::poc::reporter: PoC submission failed error=error sending request for url (https://hardwareapi.example/PoC/{key}/hardware)"
        );
        let out = scrub_line(&line);
        assert!(
            !out.to_ascii_lowercase().contains(&body(key)),
            "{key} survived: {out}"
        );
    }
}

/// What must survive: the rest of each line, the address as first4…last4,
/// and `FEM-` names that are not keys (the firewall rules FEM creates).
#[test]
fn the_rest_of_the_line_and_non_key_fem_names_survive() {
    let out = scrub_line(&logged_shapes(PRODUCTION)[1]);
    assert!(
        out.contains(r#"from="0.4.29" to="0.4.34""#),
        "over-redacted: {out}"
    );
    let out = scrub_line(&logged_shapes(PRODUCTION)[4]);
    assert!(out.ends_with("slot=17"), "over-redacted: {out}");

    let out = scrub_line(&format!("wallet {ADDR} miner_key={PRODUCTION}"));
    assert!(out.contains("YTC4…CHMA") && !out.contains(ADDR), "{out}");

    for line in [
        "INFO fry_edge_miner::integrations::firewall: Firewall rule already matches binary path rule=FEM-FryNode",
        "INFO fry_edge_miner::integrations::firewall: Firewall rule missing — creating rule=FEM-OlostepBrowser",
        // 33 characters: the validator's shape is exactly 32, so outside a
        // `miner_key` field this is not a key.
        "artifact FEM-Z21JQG5PJG5GJIX4PRXIEONBXVXQCQ2UX staged",
    ] {
        assert_eq!(scrub_line(line), line, "not a miner key: {line}");
    }
}
