//! Per-device node key tests. Synthetic values only; key values are never
//! printed in assertion messages.
use super::{fill_local_key, is_valid_key, load_or_create_key, read_key, KEY_FILE, KEY_PREFIX};
use crate::api::types::CredentialInfo;

fn creds(tok: Option<&str>) -> CredentialInfo {
    serde_json::from_value(serde_json::json!({
        "miner_key": "FEM-NKTEST",
        "diiisco_bearer_token": tok,
    }))
    .expect("deserialises")
}

fn env_override_present() -> bool {
    std::env::var("DIIISCO_BEARER_TOKEN").is_ok_and(|v| !v.is_empty())
}

fn valid_key() -> String {
    format!("{}{}", KEY_PREFIX, "ab".repeat(32))
}

#[test]
fn key_is_created_once_and_persists() {
    let d = tempfile::tempdir().unwrap();
    let k1 = load_or_create_key(d.path()).unwrap();
    assert!(is_valid_key(&k1), "first key must be valid");
    let k2 = load_or_create_key(d.path()).unwrap();
    assert!(k1 == k2, "second call must return the same key");
    let on_disk = std::fs::read_to_string(d.path().join(KEY_FILE)).unwrap();
    assert!(on_disk.trim() == k1, "file content must equal the key");
}

#[test]
fn two_devices_get_different_keys() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let ka = load_or_create_key(a.path()).unwrap();
    let kb = load_or_create_key(b.path()).unwrap();
    assert!(is_valid_key(&ka) && is_valid_key(&kb));
    assert!(ka != kb, "independent devices must not share a key");
}

#[test]
fn key_format_is_prefix_plus_64_lower_hex_and_sed_safe() {
    let d = tempfile::tempdir().unwrap();
    let k = load_or_create_key(d.path()).unwrap();
    assert!(k.starts_with(KEY_PREFIX));
    assert_eq!(k.len(), KEY_PREFIX.len() + 64);
    assert!(is_valid_key(&k));
    assert!(k
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
    assert!(!k.contains(['#', '"', '\\', '&', '/', '|']));
    assert!(!k.chars().any(char::is_whitespace));
    assert!(!is_valid_key(""));
    assert!(!is_valid_key(KEY_PREFIX));
    assert!(!is_valid_key(&format!("{}{}", KEY_PREFIX, "AB".repeat(32))));
    assert!(!is_valid_key(&format!("{}{}", KEY_PREFIX, "ab".repeat(31))));
}

#[test]
fn a_valid_existing_key_is_never_overwritten() {
    let d = tempfile::tempdir().unwrap();
    let pre = valid_key();
    std::fs::write(d.path().join(KEY_FILE), format!("{pre}\n")).unwrap();
    let got = load_or_create_key(d.path()).unwrap();
    assert!(got == pre, "existing valid key must be returned unchanged");
    let on_disk = std::fs::read_to_string(d.path().join(KEY_FILE)).unwrap();
    assert!(on_disk.trim() == pre, "file must be left as it was");
}

#[test]
fn a_corrupt_key_file_is_replaced_with_a_valid_key() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join(KEY_FILE), "garbage").unwrap();
    let got = load_or_create_key(d.path()).unwrap();
    assert!(is_valid_key(&got));
    let on_disk = std::fs::read_to_string(d.path().join(KEY_FILE)).unwrap();
    assert!(on_disk.trim() == got, "file must now hold the new key");
    assert!(is_valid_key(on_disk.trim()));
}

#[test]
fn read_key_never_creates() {
    let d = tempfile::tempdir().unwrap();
    assert!(read_key(d.path()).is_none());
    assert!(!d.path().join(KEY_FILE).exists());
    std::fs::write(d.path().join(KEY_FILE), "garbage").unwrap();
    assert!(read_key(d.path()).is_none(), "invalid content is not a key");
    let pre = valid_key();
    std::fs::write(d.path().join(KEY_FILE), &pre).unwrap();
    assert!(read_key(d.path()).as_deref() == Some(pre.as_str()));
}

#[test]
fn env_override_and_served_field_win_over_the_local_key() {
    let d = tempfile::tempdir().unwrap();

    let mut c = creds(None);
    fill_local_key(&mut c, Some("nk-synthetic-env".into()), d.path()).unwrap();
    assert!(
        c.diiisco_bearer_token.is_none(),
        "env wins, creds untouched"
    );

    let mut c = creds(Some("nk-synthetic-cred"));
    fill_local_key(&mut c, None, d.path()).unwrap();
    assert!(c.diiisco_bearer_token.as_deref() == Some("nk-synthetic-cred"));

    assert!(
        !d.path().join(KEY_FILE).exists(),
        "no key file when a token is supplied"
    );
}

#[test]
fn without_env_or_field_the_local_key_fills_the_gate() {
    if env_override_present() {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let mut c = creds(None);
    fill_local_key(&mut c, None, d.path()).unwrap();
    let got = super::super::require_diiisco_bearer(&c).expect("gate must pass");
    let on_disk = std::fs::read_to_string(d.path().join(KEY_FILE)).unwrap();
    assert!(is_valid_key(&got));
    assert!(got == on_disk.trim(), "gate token must be the file key");

    let mut c = creds(Some(""));
    fill_local_key(&mut c, Some(String::new()), d.path()).unwrap();
    assert!(c.diiisco_bearer_token.as_deref() == Some(on_disk.trim()));
}

#[test]
fn the_bare_gate_still_skips_without_a_token() {
    if env_override_present() {
        return;
    }
    let err = super::super::require_diiisco_bearer(&creds(None)).expect_err("must skip");
    let msg = err.to_string();
    assert!(
        msg.starts_with("DIIISCO_BEARER_TOKEN not configured"),
        "{msg}"
    );
}

#[cfg(unix)]
#[test]
fn key_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    load_or_create_key(d.path()).unwrap();
    let mode = std::fs::metadata(d.path().join(KEY_FILE))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn install_and_start_fill_before_each_gate() {
    let src = include_str!("diiisco.rs");
    let body = src.split("#[cfg(test)]").next().unwrap();
    let fills: Vec<usize> = body
        .match_indices("fill_local_key(")
        .map(|(i, _)| i)
        .collect();
    let gates: Vec<usize> = body
        .match_indices("require_diiisco_bearer(&creds)?")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(fills.len(), 2, "install() and start() each fill");
    assert_eq!(gates.len(), 2, "install() and start() each gate");
    assert!(
        fills[0] < gates[0] && gates[0] < fills[1] && fills[1] < gates[1],
        "order must be fill1 < gate1 < fill2 < gate2"
    );
}

#[test]
fn upgrade_never_rebuilds_an_existing_image() {
    let src = include_str!("diiisco.rs");
    let body = src.split("#[cfg(test)]").next().unwrap();
    let build = concat!("\"compose\", ", "\"build\"");
    assert_eq!(body.matches(build).count(), 2);
    // H1: superseded by operator ruling S2 (2026-10-13). The start() build is
    // now gated on "image missing OR plan == BuildNew" inside the locked
    // sequence, so the old single-literal gate no longer exists.
    assert_eq!(
        body.matches("diiisco_rebuild::run_node_sequence(").count(),
        1
    );
    assert_eq!(
        body.matches("diiisco_rebuild::after_fresh_build(").count(),
        1
    );
    let seq = include_str!("diiisco_rebuild.rs");
    assert_eq!(seq.matches("let must_build = !have_image ||").count(), 1);
    assert_eq!(seq.matches("ops.build(").count(), 2);
}

#[test]
fn node_api_is_published_on_loopback_only() {
    let compose = include_str!("diiisco_deploy/docker-compose.yml");
    assert!(compose.contains("\"127.0.0.1:8181:8181\""));
    assert!(!compose.lines().any(|l| l.trim() == "- \"8181:8181\""));
}

#[test]
fn concurrent_first_calls_agree_on_one_key() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().to_path_buf();
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let p = path.clone();
            std::thread::spawn(move || load_or_create_key(&p).unwrap())
        })
        .collect();
    let keys: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(keys.iter().all(|k| is_valid_key(k)));
    assert!(keys.iter().all(|k| *k == keys[0]), "all callers agree");
    let on_disk = std::fs::read_to_string(path.join(KEY_FILE)).unwrap();
    assert!(on_disk.trim() == keys[0]);
}
