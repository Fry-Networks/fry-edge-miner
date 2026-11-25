//! U6 RED->GREEN tests for FEM-DISCORD-DUPLICATE-KEY-CONFIG.

const DEVICE_RS: &str = include_str!("device.rs");
const SETTINGS_RS: &str = include_str!("settings.rs");
const MAIN_RS: &str = include_str!("../main.rs");

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn deregister_and_local_switch_clear_pending_install_id_and_last_reported_version() {
    let device_code = code_only(DEVICE_RS);
    let helper_at = device_code
        .find("pub(crate) fn clear_local_identity_in(")
        .expect("clear_local_identity_in must exist");
    let dereg_at = device_code
        .find("pub async fn deregister_device(")
        .expect("deregister_device must exist");
    let dereg_end = device_code[dereg_at..]
        .find("pub async fn set_device_name(")
        .map(|e| dereg_at + e)
        .unwrap();
    let clear_region = &device_code[helper_at..dereg_end];
    assert!(
        clear_region.contains("pending_install_id"),
        "clear_local_identity_in / deregister_device must clear pending_install_id"
    );
    assert!(
        clear_region.contains("last_reported_version"),
        "clear_local_identity_in / deregister_device must clear last_reported_version"
    );
    assert!(
        device_code[dereg_at..dereg_end].contains("clear_local_identity_in("),
        "deregister_device must delegate local cleanup to clear_local_identity_in"
    );
    assert!(
        device_code.contains("pub async fn switch_miner_key_local("),
        "device.rs must expose switch_miner_key_local"
    );
    assert!(
        code_only(SETTINGS_RS).contains("pub async fn get_config_paths("),
        "settings.rs must expose get_config_paths"
    );
    let main_code = code_only(MAIN_RS);
    assert!(
        main_code.contains("commands::device::switch_miner_key_local,"),
        "main.rs must register switch_miner_key_local"
    );
    assert!(
        main_code.contains("commands::settings::get_config_paths,"),
        "main.rs must register get_config_paths"
    );
}

#[test]
fn clear_local_identity_clears_all_copies_and_prevents_resurrection() {
    let tmp = tempfile::tempdir().unwrap();
    let primary_dir = tmp.path().join("primary");
    let roaming_file = tmp.path().join("roaming").join("fem_config.json");
    let store =
        crate::config::store::ConfigStore::new(primary_dir.clone(), Some(roaming_file.clone()));
    store
        .update(|cfg| {
            cfg.miner_key = Some("FEM-DUPLICATE-KEY-TEST".to_string());
            cfg.wallet_address = Some("A".repeat(58));
            cfg.install_id = Some("inst-1".to_string());
            cfg.pending_install_id = Some("pend-1".to_string());
            cfg.device_token = Some("tok-1".to_string());
            cfg.last_reported_version = Some("0.4.40".to_string());
        })
        .unwrap();
    assert!(roaming_file.exists());
    assert!(store.backup_path().exists());

    let api_client =
        crate::api::client::ApiClient::new("https://example.invalid".to_string(), "tok-1".into());
    super::device::clear_local_identity_in(&store, &api_client).unwrap();

    let after = store.get();
    assert_eq!(after.miner_key, None);
    assert_eq!(after.wallet_address, None);
    assert_eq!(after.install_id, None);
    assert_eq!(after.pending_install_id, None);
    assert_eq!(after.device_token, None);
    assert_eq!(after.last_reported_version, None);

    // Simulate deleting primary config like robhor did: reload must NOT resurrect old key.
    std::fs::remove_file(store.primary_path()).unwrap();
    let reloaded = crate::config::store::ConfigStore::new(primary_dir, Some(roaming_file));
    assert_eq!(
        reloaded.get().miner_key,
        None,
        "backup/roaming must not resurrect cleared miner_key"
    );
}
