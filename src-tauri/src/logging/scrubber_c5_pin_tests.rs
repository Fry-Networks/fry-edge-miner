//! c5 F7 pins for lens-1 survivors RC13-SCR-a/b/c: the request-path rule in
//! `redact_miner_key`. `scrubber_bl3_url_key_tests.rs` only ever puts the key
//! inside an `https://` URL and before a `/`, so dropping the rule's `(?i)`,
//! its `poc` route, or the `)` in its value class passed every test.
//!
//! The key is a kept legacy one (too short for the SHAPE rule) and no
//! `miner_key` field sits beside it, so only the path rule can redact it.
//! Synthetic.

use super::*;

const KEY: &str = "FEM-SHORTKEY123";

/// PoC submissions go to `/PoC/{key}/hardware` (capital P and C), and an API
/// decode error names that path bare, with no scheme in front. Only the
/// route arm can catch it, and only case-insensitively (RC13-SCR-a drops
/// `(?i)`, RC13-SCR-b drops `poc`).
#[test]
fn a_kept_key_in_a_bare_poc_path_is_redacted() {
    let line = format!(
        "2026-09-28T05:00:00Z  WARN fry_edge_miner::poc::reporter: PoC submission failed \
         error=Failed to decode response from /PoC/{KEY}/hardware (HTTP 200): expected value \
         at line 1 column 1"
    );
    let out = scrub_line(&line);
    assert!(!out.contains("SHORTKEY123"), "{out}");
    assert!(
        out.contains("from /PoC/FEM-[REDACTED]/hardware (HTTP 200)"),
        "{out}"
    );
}

/// reqwest prints `error sending request for url (URL)`. When the key ends
/// the URL, as in the device-credentials read `/credentials/{key}`, the `)`
/// closes the parenthesis and is not part of the key (RC13-SCR-c lets the
/// key swallow it).
#[test]
fn the_parenthesis_after_a_key_that_ends_a_url_survives() {
    let line = format!(
        "2026-09-28T05:00:01Z  WARN fry_edge_miner::api: HTTP request failed: error sending \
         request for url (https://hardwareapi.frynetworks.com/credentials/{KEY})"
    );
    let out = scrub_line(&line);
    assert!(!out.contains("SHORTKEY123"), "{out}");
    assert!(
        out.ends_with("(https://hardwareapi.frynetworks.com/credentials/FEM-[REDACTED])"),
        "{out}"
    );
}
