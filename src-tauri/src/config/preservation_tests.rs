//! B3: unknown keys in fem_config.json must survive a load → save cycle.
//!
//! `ConfigStore::save_to_disk` serializes the whole `FemConfig` struct over
//! the file, so anything serde did not recognise on load was silently dropped
//! the next time any setting changed — a user (or a support runbook) adding
//! e.g. `iagon_config` would watch it disappear the first time they toggled an
//! integration.
//!
//! These tests exercise the serde layer directly rather than the store: the
//! store's job (atomic write, backups, quarantine) is unchanged, and the loss
//! happened entirely in the derive.

use super::FemConfig;

/// A config file with two keys this build has never heard of.
const USER_EDITED: &str = r#"{
  "miner_key": "FEM-15CCB0C7A857E62200373CCF72EAA7D4",
  "wallet_address": "TESTWALLETADDRESSTESTWALLETADDRESSTESTWALLETADDRESSTESTWA",
  "integrations_enabled": { "mysterium": true },
  "api_base_url": "https://hardwareapi.frynetworks.com",
  "iagon_config": { "storage_gb": 900, "node_name": "fry-iagon-01" },
  "experimental_flag": true
}"#;

fn round_trip(json: &str) -> serde_json::Value {
    let cfg: FemConfig = serde_json::from_str(json).expect("config should deserialize");
    let out = serde_json::to_string_pretty(&cfg).expect("config should serialize");
    serde_json::from_str(&out).expect("output should be valid JSON")
}

#[test]
fn an_unknown_user_added_key_survives_a_load_save_round_trip() {
    let out = round_trip(USER_EDITED);
    assert_eq!(
        out.get("iagon_config"),
        Some(&serde_json::json!({ "storage_gb": 900, "node_name": "fry-iagon-01" })),
        "an unknown key was dropped by the round trip; got: {}",
        out
    );
    assert_eq!(
        out.get("experimental_flag"),
        Some(&serde_json::Value::Bool(true)),
        "an unknown scalar key was dropped by the round trip; got: {}",
        out
    );
}

#[test]
fn the_known_fields_still_round_trip_unchanged() {
    let out = round_trip(USER_EDITED);
    assert_eq!(
        out.get("miner_key").and_then(|v| v.as_str()),
        Some("FEM-15CCB0C7A857E62200373CCF72EAA7D4")
    );
    assert_eq!(
        out.get("api_base_url").and_then(|v| v.as_str()),
        Some("https://hardwareapi.frynetworks.com")
    );
    assert_eq!(
        out.get("integrations_enabled"),
        Some(&serde_json::json!({ "mysterium": true }))
    );
}

#[test]
fn the_api_token_is_still_never_written_back_to_disk() {
    // api_token is `skip_serializing` on purpose. Capturing unknown keys must
    // not become a back door that writes it into the file: it is a NAMED
    // field, so it has to be consumed by the named field and never land in
    // the catch-all.
    let with_token = r#"{
  "miner_key": null,
  "wallet_address": null,
  "integrations_enabled": {},
  "api_base_url": "https://hardwareapi.frynetworks.com",
  "api_token": "token-value-used-only-by-this-test"
}"#;
    let cfg: FemConfig = serde_json::from_str(with_token).expect("config should deserialize");
    assert_eq!(
        cfg.api_token, "token-value-used-only-by-this-test",
        "the named field must still receive the value"
    );
    let out = serde_json::to_string_pretty(&cfg).expect("config should serialize");
    assert!(
        !out.contains("api_token"),
        "api_token must never be serialized back to disk; output was: {}",
        out
    );
    assert!(
        !out.contains("token-value-used-only-by-this-test"),
        "the token value leaked into the saved config; output was: {}",
        out
    );
}

#[test]
fn a_config_with_no_unknown_keys_gains_nothing() {
    let minimal = r#"{
  "miner_key": null,
  "wallet_address": null,
  "integrations_enabled": {},
  "api_base_url": "https://hardwareapi.frynetworks.com"
}"#;
    let out = round_trip(minimal);
    let obj = out.as_object().expect("object");
    // Every key present must be a field this build knows about.
    assert!(
        obj.contains_key("start_on_boot") && obj.contains_key("auto_update"),
        "known defaults should still be written: {}",
        out
    );
    assert!(!obj.contains_key("extra"), "the catch-all must stay flattened, not nested: {}", out);
}
