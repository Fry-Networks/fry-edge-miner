//! U6 RED->GREEN tests for FEM-DISCORD-REINSTALL-CRASH.

use std::panic::{catch_unwind, AssertUnwindSafe};

const SENTINEL_RS: &str = include_str!("integrations/sentinel.rs");
const DOWNLOAD_RS: &str = include_str!("integrations/download.rs");

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn config_store_survives_poisoned_lock_without_panicking() {
    let tmp = tempfile::tempdir().unwrap();
    let store = crate::config::store::ConfigStore::new(tmp.path().to_path_buf(), None);
    let _ = store.update(|cfg| cfg.miner_key = Some("FEM-POISON-TEST".to_string()));

    let poisoned = catch_unwind(AssertUnwindSafe(|| {
        let _ = store.update(|_| panic!("simulated panic holding ConfigStore write lock"));
    }));
    assert!(poisoned.is_err());

    let recovered = catch_unwind(AssertUnwindSafe(|| store.get()));
    assert!(
        recovered.is_ok(),
        "ConfigStore::get() must recover from a poisoned RwLock instead of panicking"
    );
    assert_eq!(
        recovered.unwrap().miner_key.as_deref(),
        Some("FEM-POISON-TEST")
    );
}

#[test]
fn sentinel_and_download_eliminate_runtime_expect_panics() {
    let sentinel_code = code_only(SENTINEL_RS);
    assert!(
        !sentinel_code.contains(".expect(\"no local data dir\")"),
        "sentinel.rs must not panic with .expect(\"no local data dir\")"
    );

    let download_code = code_only(DOWNLOAD_RS);
    let bc_at = download_code
        .find("fn build_client(")
        .expect("build_client must exist");
    let bc_end = download_code[bc_at..]
        .find("\n}")
        .map(|e| bc_at + e)
        .unwrap();
    let bc_body = &download_code[bc_at..bc_end];
    assert!(
        !bc_body.contains(".expect("),
        "download.rs build_client must return Result instead of calling .expect(): {bc_body}"
    );
}

#[test]
fn check_update_outcome_on_launch_cleans_stale_update_failed_txt_on_success() {
    let tmp = tempfile::tempdir().unwrap();
    let state_path = tmp.path().join("update-state.json");
    std::fs::write(
        &state_path,
        r#"{"from":"0.4.39","to":"0.4.40","ts":1700000000}"#,
    )
    .unwrap();
    let failed_txt = tmp.path().join("update-failed.txt");
    std::fs::write(&failed_txt, "previous update failed").unwrap();

    let outcome = crate::updater_auto::check_update_outcome_on_launch(tmp.path(), "0.4.40");
    assert_eq!(outcome, Some(crate::updater_auto::UpdateOutcome::Succeeded));
    assert!(
        !failed_txt.exists(),
        "stale update-failed.txt must be cleaned up on a successful update launch"
    );
}

#[test]
fn sentinel_none_local_data_dir_returns_err_without_panicking() {
    let res = catch_unwind(|| crate::integrations::sentinel::resolve_local_deploy_dir(None));
    assert!(res.is_ok(), "resolve_local_deploy_dir(None) must not panic");
    assert!(
        res.unwrap().is_err(),
        "resolve_local_deploy_dir(None) must return Err"
    );
}
