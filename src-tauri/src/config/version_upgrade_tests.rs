//! BUG 11/12: a config file written by v0.4.27 (before `last_reported_version`
//! existed) must load byte-for-byte identical on the six fields that matter
//! most for "am I still the same registered device" — install_id, miner_key,
//! device_token, wallet_address, integrations_enabled, integration_versions —
//! under a build that added new fields (v0.4.28), and a save/reload cycle
//! (simulating the app running for a while after the update) must not
//! disturb them either.

use super::FemConfig;

/// Exactly what a real, fully-registered v0.4.27 fem_config.json looks
/// like on disk — no `last_reported_version` key, since that field did
/// not exist yet.
const V0427_REGISTERED_DEVICE: &str = r#"{
  "miner_key": "FEM-15CCB0C7A857E62200373CCF72EAA7D4",
  "wallet_address": "TESTWALLETADDRESSTESTWALLETADDRESSTESTWALLETADDRESSTESTWA",
  "install_id": "install-9f2c3d4e-0427",
  "initial_setup_done": true,
  "integrations_enabled": { "mysterium": true, "space_acres": false, "titan": true },
  "integration_versions": { "mysterium": "1.4.2", "titan": "0.9.1" },
  "api_base_url": "https://hardwareapi.frynetworks.com",
  "device_token": "dev-token-abc123",
  "device_name": "GEORGE-RIG-01",
  "start_on_boot": true,
  "minimize_to_tray": true,
  "auto_update": true,
  "notifications": true,
  "myst_lan_override": false
}"#;

#[test]
fn a_v0427_config_loads_with_all_registration_fields_unchanged_under_the_current_build() {
    let cfg: FemConfig =
        serde_json::from_str(V0427_REGISTERED_DEVICE).expect("v0.4.27 config must still parse");

    assert_eq!(cfg.install_id.as_deref(), Some("install-9f2c3d4e-0427"));
    assert_eq!(cfg.miner_key.as_deref(), Some("FEM-15CCB0C7A857E62200373CCF72EAA7D4"));
    assert_eq!(cfg.device_token.as_deref(), Some("dev-token-abc123"));
    assert_eq!(
        cfg.wallet_address.as_deref(),
        Some("TESTWALLETADDRESSTESTWALLETADDRESSTESTWALLETADDRESSTESTWA")
    );
    assert_eq!(cfg.integrations_enabled.get("mysterium"), Some(&true));
    assert_eq!(cfg.integrations_enabled.get("space_acres"), Some(&false));
    assert_eq!(cfg.integrations_enabled.get("titan"), Some(&true));
    assert_eq!(cfg.integration_versions.get("mysterium").map(String::as_str), Some("1.4.2"));
    assert_eq!(cfg.integration_versions.get("titan").map(String::as_str), Some("0.9.1"));

    // A field that did not exist in 0.4.27 must default to a value that
    // forces one immediate report, never a silent "already reported".
    assert_eq!(cfg.last_reported_version, None);
}

#[test]
fn a_save_reload_cycle_after_loading_a_v0427_config_keeps_every_registration_field() {
    let cfg: FemConfig =
        serde_json::from_str(V0427_REGISTERED_DEVICE).expect("v0.4.27 config must still parse");
    let resaved = serde_json::to_string_pretty(&cfg).expect("config must re-serialize");
    let reloaded: FemConfig =
        serde_json::from_str(&resaved).expect("re-serialized config must still parse");

    assert_eq!(reloaded.install_id, cfg.install_id);
    assert_eq!(reloaded.miner_key, cfg.miner_key);
    assert_eq!(reloaded.device_token, cfg.device_token);
    assert_eq!(reloaded.wallet_address, cfg.wallet_address);
    assert_eq!(reloaded.integrations_enabled, cfg.integrations_enabled);
    assert_eq!(reloaded.integration_versions, cfg.integration_versions);
}

#[test]
fn once_a_version_is_reported_it_round_trips_through_a_save_cycle_too() {
    let mut cfg: FemConfig =
        serde_json::from_str(V0427_REGISTERED_DEVICE).expect("v0.4.27 config must still parse");
    cfg.last_reported_version = Some("0.4.28".to_string());

    let resaved = serde_json::to_string_pretty(&cfg).expect("config must re-serialize");
    assert!(
        resaved.contains("\"last_reported_version\": \"0.4.28\""),
        "last_reported_version must be persisted once set: {}",
        resaved
    );
    let reloaded: FemConfig =
        serde_json::from_str(&resaved).expect("re-serialized config must still parse");
    assert_eq!(reloaded.last_reported_version.as_deref(), Some("0.4.28"));
}
