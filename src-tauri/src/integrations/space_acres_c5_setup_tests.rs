//! D-C5-1 — SpaceAcres is Healthy, reports poa=true and earns its boost only
//! while its OWN configuration holds a reward address and at least one farm
//! AND its process is alive. Running but unconfigured, the card shows exactly
//! `Finish setup in the SpaceAcres window to start earning.` and poa is false.
//!
//! Configuration path and format are those of SpaceAcres 0.2.21 (the release
//! FEM installs; tag commit d4d04b0c4ac3f17ceefd2655becf9bcf8bde54cc):
//! src/backend/config.rs:96-114 resolves `dirs::config_local_dir()` joined
//! with `CARGO_PKG_NAME` (Cargo.toml:2, "space-acres") and "config.json";
//! config.rs:67-81 is JSON tagged `"version": "0"` with camelCase fields, of
//! which `rewardAddress` (:72) and `farms` (:75) decide the ruling.

const SETUP_TEXT: &str = "Finish setup in the SpaceAcres window to start earning.";

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The body of an `impl` method (closes at the first four-space `}` line).
fn method_body(code: &str, signature: &str) -> String {
    let at = code
        .find(signature)
        .unwrap_or_else(|| panic!("{signature} must exist"));
    let end = at + code[at..].find("\n    }\n").expect("must close");
    code[at..end].to_string()
}

#[test]
fn the_setup_text_is_the_ruling_text_exactly() {
    let src = include_str!("space_acres.rs");
    assert!(
        src.contains(&format!("\"{SETUP_TEXT}\"")),
        "the SpaceAcres setup text must be exactly {SETUP_TEXT:?}"
    );
}

#[test]
fn health_check_reads_the_configuration_on_every_call() {
    let code = code_only(include_str!("space_acres.rs"));
    let body = method_body(&code, "async fn health_check(&self) -> HealthStatus {");
    assert!(
        body.contains("space_acres_configured()"),
        "health_check never consults the SpaceAcres configuration, so an \
         unconfigured SpaceAcres reads Healthy:\n{body}"
    );
}

#[test]
fn poa_requires_the_configuration() {
    let code = code_only(include_str!("space_acres.rs"));
    let body = method_body(&code, "fn collect_poc_data(&self) -> PocGateData {");
    assert!(
        body.contains("space_acres_configured()"),
        "poa follows the process alone, so an unconfigured SpaceAcres reports poa=true:\n{body}"
    );
}

#[test]
fn the_configuration_path_is_upstreams() {
    let code = code_only(include_str!("space_acres.rs"));
    let at = code
        .find("fn space_acres_config_path()")
        .expect("the SpaceAcres configuration path must be resolved in one place");
    let body = &code[at..at + code[at..].find("\n}\n").expect("must close")];
    for needle in [
        "dirs::config_local_dir()",
        "\"space-acres\"",
        "\"config.json\"",
    ] {
        assert!(
            body.contains(needle),
            "upstream 0.2.21 reads config_local_dir()/space-acres/config.json; missing {needle}:\n{body}"
        );
    }
}

#[test]
fn a_supervisor_restart_never_closes_an_unconfigured_setup_window() {
    let code = code_only(include_str!("space_acres.rs"));
    let body = method_body(&code, "async fn stop_for_restart(&self) -> Result<()> {");
    assert!(
        body.contains("space_acres_configured()") && body.contains("self.stop().await"),
        "a restart must leave a running, unconfigured SpaceAcres (its setup window) alone \
         and stop anything else:\n{body}"
    );
}

#[test]
fn the_configuration_is_read_fresh_on_every_call() {
    let code = code_only(include_str!("space_acres.rs"));
    let at = code
        .find("fn space_acres_configured() -> bool {")
        .expect("space_acres_configured must exist");
    let body = &code[at..at + code[at..].find("\n}\n").expect("must close")];
    assert!(
        body.contains("configured_at(&"),
        "space_acres_configured must read the file:\n{body}"
    );
    for cache in [
        "static",
        "OnceLock",
        "LazyLock",
        "get_or_init",
        "thread_local",
    ] {
        assert!(
            !body.contains(cache),
            "a cached answer never sees the owner finish setup ({cache}):\n{body}"
        );
    }
    let at = code
        .find("fn configured_at(path: &std::path::Path) -> bool {")
        .expect("configured_at must exist");
    let body = &code[at..at + code[at..].find("\n}\n").expect("must close")];
    assert!(
        body.contains("std::fs::read_to_string(path)"),
        "configured_at must read the file each time:\n{body}"
    );
}

use super::{
    configured_at, configured_from, health_from, poa_from, restart_spares_the_setup_window,
    space_acres_config_path, SpaceAcresIntegration, SETUP_REQUIRED_REASON,
};
use crate::integrations::HealthStatus;

/// A labelled decoy in upstream 0.2.21's own layout (`to_string_pretty` of
/// `RawConfig::V0`, config.rs:67-81). Not a real account.
const UPSTREAM_SHAPED: &str = r#"{
  "version": "0",
  "rewardAddress": "fem-qa-decoy-reward-address",
  "nodePath": "C:\\Users\\owner\\AppData\\Local\\space-acres\\node",
  "farms": [
    {
      "path": "D:\\fem-qa-decoy-farm",
      "size": "100 GiB"
    }
  ],
  "reducePlottingCpuLoad": false,
  "network": {
    "substrate_port": 30333,
    "subspace_port": 30433,
    "faster_networking": false
  }
}"#;

#[test]
fn the_configured_predicate_table() {
    let farm = r#"{"path": "D:\\farm", "size": "100 GiB"}"#;
    let rows: [(&str, String, bool); 12] = [
        ("empty file", String::new(), false),
        ("not JSON", "not json".into(), false),
        ("empty object", "{}".into(), false),
        (
            "upstream's default RawConfig (empty address, no farms)",
            r#"{"version":"0","rewardAddress":"","nodePath":"","farms":[]}"#.into(),
            false,
        ),
        (
            "empty address, one farm",
            format!(r#"{{"rewardAddress":"","farms":[{farm}]}}"#),
            false,
        ),
        (
            "whitespace address, one farm",
            format!(r#"{{"rewardAddress":"   ","farms":[{farm}]}}"#),
            false,
        ),
        (
            "address, no farms",
            r#"{"rewardAddress":"st-decoy","farms":[]}"#.into(),
            false,
        ),
        (
            "address, farms missing",
            r#"{"rewardAddress":"st-decoy"}"#.into(),
            false,
        ),
        (
            "address, farms not a list",
            r#"{"rewardAddress":"st-decoy","farms":"D:\\farm"}"#.into(),
            false,
        ),
        (
            "address not a string",
            format!(r#"{{"rewardAddress":7,"farms":[{farm}]}}"#),
            false,
        ),
        (
            "address and one farm",
            format!(r#"{{"rewardAddress":"st-decoy","farms":[{farm}]}}"#),
            true,
        ),
        ("upstream-shaped, one farm", UPSTREAM_SHAPED.into(), true),
    ];
    for (what, contents, expected) in rows {
        assert_eq!(configured_from(&contents), expected, "{what}: {contents}");
    }
    let two_farms = format!(r#"{{"rewardAddress":"st-decoy","farms":[{farm},{farm}]}}"#);
    assert!(configured_from(&two_farms), "two farms is configured");
}

#[test]
fn a_missing_configuration_file_is_not_configured() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!configured_at(&dir.path().join("config.json")));
}

#[test]
fn the_configuration_path_resolves_like_upstream() {
    let expected = dirs::config_local_dir().map(|d| d.join("space-acres").join("config.json"));
    assert!(
        expected.is_some(),
        "control: this host has a local config dir"
    );
    assert_eq!(space_acres_config_path(), expected);
}

#[test]
fn the_health_table() {
    let setup = HealthStatus::Unhealthy(SETUP_TEXT.to_string());
    // (running, configured) -> health
    assert_eq!(health_from(false, false), HealthStatus::Stopped);
    assert_eq!(health_from(false, true), HealthStatus::Stopped);
    assert_eq!(health_from(true, false), setup);
    assert_eq!(health_from(true, true), HealthStatus::Healthy);
    assert_eq!(SETUP_REQUIRED_REASON, SETUP_TEXT);
}

#[test]
fn poa_only_while_running_and_configured() {
    assert!(!poa_from(false, false));
    assert!(!poa_from(false, true));
    assert!(!poa_from(true, false));
    assert!(poa_from(true, true));
}

#[test]
fn a_restart_spares_only_a_running_unconfigured_space_acres() {
    // (configured, running)
    assert!(restart_spares_the_setup_window(false, true));
    assert!(!restart_spares_the_setup_window(true, true));
    assert!(!restart_spares_the_setup_window(false, false));
    assert!(!restart_spares_the_setup_window(true, false));
}

/// A long-lived stand-in for the SpaceAcres process, on every CI platform.
fn stand_in() -> std::process::Child {
    #[cfg(not(target_os = "windows"))]
    let child = std::process::Command::new("sleep").arg("30").spawn();
    #[cfg(target_os = "windows")]
    let child = std::process::Command::new("ping")
        .args(["-n", "30", "127.0.0.1"])
        .spawn();
    child.expect("spawn a stand-in process")
}

/// F1: a configuration written while SpaceAcres runs turns it Healthy on the
/// next evaluation — the supervisor then has nothing to restart, and while
/// the owner was still in setup a restart left the process alone.
#[test]
fn a_configuration_written_while_space_acres_runs_turns_it_healthy_without_a_restart() {
    use crate::supervisor::health::{recovery_action, RecoveryAction};
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.json");
    let sa = SpaceAcresIntegration::default();
    *sa.child.lock().unwrap() = Some(stand_in());
    assert!(sa.is_running(), "control: the tracked stand-in runs");

    // The owner is still in the setup window.
    assert_eq!(
        health_from(sa.is_running(), configured_at(&config)),
        HealthStatus::Unhealthy(SETUP_TEXT.to_string())
    );
    assert!(!poa_from(sa.is_running(), configured_at(&config)));
    assert!(restart_spares_the_setup_window(
        configured_at(&config),
        sa.is_running()
    ));

    // The owner finishes the setup window: SpaceAcres writes its config.
    std::fs::write(&config, UPSTREAM_SHAPED).unwrap();
    assert_eq!(
        health_from(sa.is_running(), configured_at(&config)),
        HealthStatus::Healthy
    );
    assert!(poa_from(sa.is_running(), configured_at(&config)));

    assert_eq!(
        recovery_action(
            &health_from(sa.is_running(), configured_at(&config)),
            true,
            0,
            6
        ),
        RecoveryAction::None,
        "a configured, running SpaceAcres must give the supervisor nothing to restart"
    );
    assert!(
        !restart_spares_the_setup_window(configured_at(&config), sa.is_running()),
        "once configured, a restart is an ordinary one again"
    );
    let tracked = sa.child.lock().unwrap().take();
    if let Some(mut c) = tracked {
        let _ = c.kill();
        let _ = c.wait();
    }
}
