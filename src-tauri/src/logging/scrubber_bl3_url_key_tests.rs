//! BUG LOOP 3 — a kept legacy miner key in URL paths and escaped JSON.
//!
//! `resolve_stored_miner_key` keeps a stored key that fails today's
//! validator, and FEM builds request paths from the stored string as is:
//! `/credentials/{key}`, `/credentials/{key}/verified`, `/PoC/{key}/hardware`,
//! `/installations/{key}/leases/…`. Those reach fem.log and the bundle inside
//! reqwest errors ("… for url (…)") and decode errors ("… from {path}"), with
//! no `miner_key` field to key on — and a Debug-printed body escapes its JSON
//! quotes. Keys here are synthetic.

use super::*;

/// The shape an older release could have stored (`device.rs`), and one with
/// dashes of its own.
const LEGACY: [&str; 2] = ["FEM-SHORTKEY123", "FEM-OLD-KEY-2024"];

/// The lines the key reaches outside any `miner_key` field.
fn leaking_lines(key: &str) -> Vec<String> {
    vec![
        format!("2026-09-24T07:00:00Z  WARN fry_edge_miner: PoC submission failed — retrying once error=HTTP request failed: error sending request for url (https://hardwareapi.frynetworks.com/PoC/{key}/hardware)"),
        format!("2026-09-24T07:00:01Z  WARN fry_edge_miner::integrations::fryvpn: Could not fetch device wallet - starting fryDVPN without it error=Failed to decode response from /credentials/{key}"),
        format!("2026-09-24T07:00:02Z  WARN fry_edge_miner::api: HTTP request failed: error sending request for url (https://hardwareapi.frynetworks.com/credentials/{key}/verified)"),
        format!("2026-09-24T07:00:03Z  WARN fry_edge_miner::api: GET /installations/{key}/leases/current returned 503"),
        format!("2026-09-24T07:00:04Z  WARN fry_edge_miner::poc::reporter: PoC submission failed miner_key=\"{key}\" error=HTTP request failed: error sending request for url (https://hardwareapi.example/PoC/{key}/hardware)"),
        format!(r#"2026-09-24T07:00:05Z DEBUG fry_edge_miner::api: request body="{{\"miner_key\":\"{key}\"}}""#),
        format!(r#"2026-09-24T07:00:06Z DEBUG fry_edge_miner::api: request={{\"miner_key\": \"{key}\", \"slot\": 17}}"#),
        // Any URL, not only today's routes.
        format!("2026-09-24T07:00:07Z  WARN fry_edge_miner::api: HTTP request failed: error sending request for url (https://hardwareapi.frynetworks.com/v3/devices/{key}/status)"),
    ]
}

fn assert_legacy_key_gone(scrub: fn(&str) -> String, path: &str) {
    for key in LEGACY {
        // Every fragment of the key, so a partial redaction that leaves
        // `-KEY-2024` behind counts as the leak it is.
        let fragments: Vec<&str> = key.trim_start_matches("FEM-").split('-').collect();
        for line in leaking_lines(key) {
            let out = scrub(&line);
            for fragment in &fragments {
                assert!(
                    !out.contains(fragment),
                    "{path}: {fragment:?} of the kept legacy key {key} survived: {out}"
                );
            }
            assert!(out.contains("FEM-[REDACTED]"), "{path}: {out}");
            assert_eq!(scrub(&out), out, "{path}: must stay idempotent");
        }
    }
}

#[test]
fn a_kept_legacy_key_in_a_url_path_or_escaped_json_is_redacted() {
    assert_legacy_key_gone(scrub_line, "scrub_line");
}

#[test]
fn the_partner_scrubber_redacts_it_too() {
    assert_legacy_key_gone(scrub_partner_line, "scrub_partner_line");
}

/// Only the key's own segment goes: the route around it stays readable.
#[test]
fn the_route_around_the_key_survives() {
    let lines = leaking_lines("FEM-SHORTKEY123");
    let out = scrub_line(&lines[0]);
    assert!(out.contains("/PoC/FEM-[REDACTED]/hardware)"), "{out}");
    let out = scrub_line(&lines[2]);
    assert!(
        out.contains("/credentials/FEM-[REDACTED]/verified)"),
        "{out}"
    );
    let out = scrub_line(&lines[3]);
    assert!(
        out.contains("/installations/FEM-[REDACTED]/leases/current returned 503"),
        "{out}"
    );
    let out = scrub_line(&lines[6]);
    assert!(out.contains(r#"\"slot\": 17"#), "{out}");
}

/// `FEM-` names outside a request path — the firewall rules FEM creates, a
/// directory — are not keys at all.
#[test]
fn fem_names_outside_a_url_path_are_not_keys() {
    for line in [
        "INFO fry_edge_miner::integrations::firewall: Firewall rule already matches binary path rule=FEM-FryNode",
        "INFO fry_edge_miner::integrations::firewall: Firewall rule missing — creating rule=FEM-OlostepBrowser",
        r#"netsh advfirewall firewall add rule name="FEM-FryNode" dir=in action=allow"#,
        // A filesystem path is never a request path: partner logs depend on
        // their paths surviving the scrubber verbatim.
        "staged at /tmp/FEM-build-7/out/frynode",
    ] {
        assert_eq!(scrub_line(line), line, "not a miner key: {line}");
        assert_eq!(scrub_partner_line(line), line, "not a miner key: {line}");
    }
}
