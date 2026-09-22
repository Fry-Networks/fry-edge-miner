//! B5: "configs captured from real 0.4.29 / 0.4.31 / 0.4.33 installs migrate to
//! the current schema with defaults filled and user values preserved,
//! idempotently (migrate twice -> identical); one test per fixture."
//!
//! Nothing of the sort existed: no migration entry point, no fixtures, no
//! per-fixture test, no idempotency test. `grep` for
//! schema_version/config_version/migrate_config across src-tauri/src returned
//! one unrelated comment, and the only "old version" document in the tree was a
//! hand-written v0.4.27 string inline in `version_upgrade_tests.rs`.
//!
//! FIXTURE PROVENANCE. `fixtures/fem_config_v0.4.{29,31,33}.json` are derived
//! from each shipped tag's own `config/mod.rs` — the exact key set that version
//! serialises for a registered device with a populated `integration_versions`,
//! read out of `git show v0.4.29:src-tauri/src/config/mod.rs` and the same for
//! v0.4.31 and v0.4.33. `storage_dir` therefore appears only in the 0.4.31 and
//! 0.4.33 fixtures (the field is introduced at v0.4.30) and
//! `debug_logging_enabled` only in 0.4.33 (introduced at v0.4.32). Identity
//! values are the repo's existing synthetic ones, never a real device's. These
//! are to be REPLACED in place by the VM captures when T1 delivers them; the
//! tests below are written against whatever the files contain, so a swap needs
//! no test change.

use super::{migrate, FemConfig};
use std::path::PathBuf;

const V0429: &str = include_str!("fixtures/fem_config_v0.4.29.json");
const V0431: &str = include_str!("fixtures/fem_config_v0.4.31.json");
const V0433: &str = include_str!("fixtures/fem_config_v0.4.33.json");

/// One migration: read a captured document, write what FEM would now save.
fn migrate_str(raw: &str) -> String {
    let cfg = migrate::parse(raw).expect("a captured config must still parse");
    migrate::to_disk_string(&cfg).expect("a parsed config must serialise")
}

fn as_object(raw: &str) -> serde_json::Map<String, serde_json::Value> {
    match serde_json::from_str::<serde_json::Value>(raw).expect("valid JSON") {
        serde_json::Value::Object(m) => m,
        other => panic!("a config must be a JSON object, got {other}"),
    }
}

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fem_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Every key the current schema always writes. A migrated config must carry all
/// of them whatever the capture was missing — that is "defaults filled".
const ALWAYS_WRITTEN: [&str; 10] = [
    "api_base_url",
    "integrations_enabled",
    "integration_versions",
    "initial_setup_done",
    "start_on_boot",
    "minimize_to_tray",
    "auto_update",
    "notifications",
    "myst_lan_override",
    "debug_logging_enabled",
];

fn assert_migrates_cleanly(label: &str, capture: &str) {
    let migrated = migrate_str(capture);
    let before = as_object(capture);
    let after = as_object(&migrated);

    for key in ALWAYS_WRITTEN {
        assert!(
            after.contains_key(key),
            "{label}: the migrated config is missing `{key}` — a default was not filled"
        );
    }

    // Every value the user's capture carried must survive byte-equal. Object
    // comparison is order-independent, so this checks content, not layout.
    for (key, value) in &before {
        assert_eq!(
            after.get(key),
            Some(value),
            "{label}: migration changed `{key}` — a user value was not preserved"
        );
    }

    assert!(
        !after.contains_key("api_token"),
        "{label}: api_token is skip_serializing and must never be written back to disk"
    );
}

#[test]
fn a_v0429_capture_migrates_with_defaults_filled_and_user_values_preserved() {
    assert_migrates_cleanly("v0.4.29", V0429);
    // The fields that did not exist at this tag must appear with their
    // defaults rather than being invented from the capture.
    let after = as_object(&migrate_str(V0429));
    assert_eq!(
        after.get("debug_logging_enabled"),
        Some(&serde_json::Value::Bool(false))
    );
    assert!(
        !after.contains_key("storage_dir"),
        "storage_dir is skip_serializing_if=none, so an upgrade must stay a \
         byte-for-byte no-op on a device that never chose a location"
    );
}

#[test]
fn a_v0431_capture_migrates_with_defaults_filled_and_user_values_preserved() {
    assert_migrates_cleanly("v0.4.31", V0431);
}

#[test]
fn a_v0433_capture_migrates_with_defaults_filled_and_user_values_preserved() {
    assert_migrates_cleanly("v0.4.33", V0433);
}

/// B5 Done-when: "idempotently (migrate twice -> identical)".
///
/// `integrations_enabled` and `integration_versions` are std `HashMap`s, and
/// std seeds every instance's `RandomState` differently — so before
/// `ordered_map`, two independently-deserialised copies of the same config
/// could serialise their entries in different orders and this assertion could
/// not hold. The loop is what makes it deterministic instead of a coin flip:
/// each iteration builds fresh maps with fresh seeds.
fn assert_migration_is_order_stable(label: &str, capture: &str) {
    let once = migrate_str(capture);
    let twice = migrate_str(&once);
    assert_eq!(once, twice, "{label}: migrating twice changed the bytes");
    for i in 0..64 {
        assert_eq!(
            migrate_str(&once),
            once,
            "{label}: config serialisation is not order-stable (iteration {i})"
        );
    }
}

#[test]
fn migrating_a_v0429_capture_twice_is_byte_identical() {
    assert_migration_is_order_stable("v0.4.29", V0429);
}

#[test]
fn migrating_a_v0431_capture_twice_is_byte_identical() {
    assert_migration_is_order_stable("v0.4.31", V0431);
}

#[test]
fn migrating_a_v0433_capture_twice_is_byte_identical() {
    assert_migration_is_order_stable("v0.4.33", V0433);
}

/// D1. `integrations_enabled` and `api_base_url` carried no serde default, so a
/// config merely MISSING one of them failed the whole parse. A failed parse is
/// quarantined and the device falls through to `FemConfig::default()` — miner
/// key and wallet gone. That contradicts `ConfigStore`'s own stated design,
/// which `storage_location.rs` states outright: "a load must never fail".
#[test]
fn a_config_missing_api_base_url_loads_with_the_default() {
    let raw = r#"{
        "miner_key": "FEM-15CCB0C7A857E62200373CCF72EAA7D4",
        "wallet_address": "TESTWALLETADDRESSTESTWALLETADDRESSTESTWALLETADDRESSTESTWA",
        "integrations_enabled": {}
    }"#;
    let cfg: FemConfig = migrate::parse(raw).expect(
        "a config missing api_base_url must load with the default, not fail the whole parse",
    );
    assert_eq!(cfg.api_base_url, "https://hardwareapi.frynetworks.com");
    assert_eq!(
        cfg.miner_key.as_deref(),
        Some("FEM-15CCB0C7A857E62200373CCF72EAA7D4")
    );
}

#[test]
fn a_config_missing_integrations_enabled_loads_empty() {
    let raw = r#"{
        "miner_key": "FEM-15CCB0C7A857E62200373CCF72EAA7D4",
        "wallet_address": "TESTWALLETADDRESSTESTWALLETADDRESSTESTWALLETADDRESSTESTWA",
        "api_base_url": "https://hardwareapi.frynetworks.com"
    }"#;
    let cfg: FemConfig = migrate::parse(raw)
        .expect("a config missing integrations_enabled must load with an empty map");
    assert!(cfg.integrations_enabled.is_empty());
    assert_eq!(
        cfg.miner_key.as_deref(),
        Some("FEM-15CCB0C7A857E62200373CCF72EAA7D4")
    );
}

/// The same defect at store level, which is where it cost users their key: the
/// file is RENAMED away, not merely skipped.
#[test]
fn a_config_missing_a_non_optional_key_is_not_quarantined_and_keeps_the_miner_key() {
    let dir = unique_dir("b5_missing_key");
    std::fs::write(
        dir.join("fem_config.json"),
        r#"{
        "miner_key": "FEM-15CCB0C7A857E62200373CCF72EAA7D4",
        "wallet_address": "TESTWALLETADDRESSTESTWALLETADDRESSTESTWALLETADDRESSTESTWA",
        "integrations_enabled": {"titan": true}
    }"#,
    )
    .unwrap();

    let store = super::store::ConfigStore::new(dir.clone(), None);

    assert_eq!(
        store.get().miner_key.as_deref(),
        Some("FEM-15CCB0C7A857E62200373CCF72EAA7D4"),
        "a missing optional key must not cost the device its identity"
    );
    let quarantined: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".corrupt."))
        .collect();
    assert!(
        quarantined.is_empty(),
        "a config missing a key with a default is not corrupt and must not be \
         quarantined: {quarantined:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// D4. The quarantine name was derived from a constant stem plus whole seconds,
/// and the primary and backup live in the SAME directory and are tried
/// microseconds apart — so both produced one identical path and `rename`
/// replaced the first with the second. The destroyed file was the primary's,
/// i.e. the only surviving copy of the identity the user is told to look at.
#[test]
fn quarantining_a_corrupt_primary_and_backup_keeps_both_artifacts() {
    let dir = unique_dir("b5_quarantine_collision");
    std::fs::write(
        dir.join("fem_config.json"),
        r#"{"miner_key":"PRIMARY-MARKER","wallet_addre"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("fem_config.backup.json"),
        r#"{"miner_key":"BACKUP-MARKER","wallet_addre"#,
    )
    .unwrap();

    let _store = super::store::ConfigStore::new(dir.clone(), None);

    let quarantined: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".corrupt."))
        .collect();
    assert_eq!(
        quarantined.len(),
        2,
        "both corrupt copies must be kept for support, not one overwritten by \
         the other: {quarantined:?}"
    );

    let bodies: Vec<String> = quarantined
        .iter()
        .map(|n| std::fs::read_to_string(dir.join(n)).unwrap())
        .collect();
    assert!(
        bodies.iter().any(|b| b.contains("PRIMARY-MARKER")),
        "the PRIMARY copy must survive — it is the one the load warning points \
         the user at: {bodies:?}"
    );
    assert!(
        bodies.iter().any(|b| b.contains("BACKUP-MARKER")),
        "the backup copy must survive too: {bodies:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The primary's artifact name must not change: support has been telling users
/// to look for exactly this file since before this fix.
#[test]
fn the_primary_quarantine_keeps_the_name_support_already_documents() {
    let dir = unique_dir("b5_quarantine_name");
    std::fs::write(dir.join("fem_config.json"), "{ truncated").unwrap();

    let _store = super::store::ConfigStore::new(dir.clone(), None);

    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".corrupt."))
        .collect();
    assert_eq!(names.len(), 1, "one corrupt copy, one artifact: {names:?}");
    assert!(
        names[0].starts_with("fem_config.corrupt.") && names[0].ends_with(".json"),
        "the primary's quarantine name must stay fem_config.corrupt.<ts>.json: {names:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Keys a NEWER FEM wrote must survive a migration run by an older one, or a
/// downgrade-then-upgrade silently drops settings. Covered at load/save level
/// by `preservation_tests`; asserted here for the migration entry point itself.
#[test]
fn a_key_this_build_does_not_recognise_survives_the_migration() {
    let raw = r#"{
        "miner_key": "FEM-15CCB0C7A857E62200373CCF72EAA7D4",
        "integrations_enabled": {},
        "api_base_url": "https://hardwareapi.frynetworks.com",
        "a_setting_from_a_future_build": {"nested": [1, 2, 3]}
    }"#;
    let after = as_object(&migrate_str(raw));
    assert_eq!(
        after.get("a_setting_from_a_future_build"),
        Some(&serde_json::json!({"nested": [1, 2, 3]})),
        "an unrecognised key must round-trip through FemConfig::extra"
    );
}
