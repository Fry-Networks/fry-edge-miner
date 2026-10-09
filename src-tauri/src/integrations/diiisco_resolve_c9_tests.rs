//! C9: token precedence and the skip path. Synthetic values only.
use super::{require_diiisco_bearer, resolve_diiisco_bearer};
use crate::api::types::CredentialInfo;

fn creds(tok: Option<&str>) -> CredentialInfo {
    serde_json::from_value(serde_json::json!({
        "miner_key": "FEM-C9TEST",
        "diiisco_bearer_token": tok,
    }))
    .expect("deserialises")
}

/// (c) env wins; creds next; otherwise None.
#[test]
fn c9_resolve_precedence() {
    let c = creds(Some("c9-synthetic-cred"));
    assert_eq!(
        resolve_diiisco_bearer(Some("c9-synthetic-env".into()), &c).as_deref(),
        Some("c9-synthetic-env")
    );
    assert_eq!(
        resolve_diiisco_bearer(None, &c).as_deref(),
        Some("c9-synthetic-cred")
    );
    assert_eq!(
        resolve_diiisco_bearer(Some(String::new()), &c).as_deref(),
        Some("c9-synthetic-cred"),
        "an empty env var must not mask the credential"
    );
    assert_eq!(resolve_diiisco_bearer(None, &creds(None)), None);
    assert_eq!(resolve_diiisco_bearer(None, &creds(Some(""))), None);
    assert_eq!(
        resolve_diiisco_bearer(Some(String::new()), &creds(None)),
        None
    );
}

/// (d) With no token the decision point errors (existing wording) instead of
/// proceeding; and in both install() and start() it sits before any
/// `docker compose build`, so the skip path never reaches docker.
#[test]
fn c9_skip_path_never_builds_without_a_token() {
    if std::env::var("DIIISCO_BEARER_TOKEN").is_ok_and(|v| !v.is_empty()) {
        return; // dev override present in this environment; pure tests above cover precedence
    }
    let err = require_diiisco_bearer(&creds(None)).expect_err("no token must skip");
    let msg = err.to_string();
    assert!(
        msg.starts_with("DIIISCO_BEARER_TOKEN not configured"),
        "{msg}"
    );

    let src = include_str!("diiisco.rs");
    let body = src.split("#[cfg(test)]").next().unwrap();
    let builds: Vec<usize> = body
        .match_indices("\"compose\", \"build\"")
        .map(|(i, _)| i)
        .collect();
    let gates: Vec<usize> = body
        .match_indices("require_diiisco_bearer(&creds)?")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(builds.len(), 2, "install() and start() each build");
    assert_eq!(
        gates.len(),
        2,
        "install() and start() each gate on the token"
    );
    for (g, b) in gates.iter().zip(&builds) {
        assert!(g < b, "token gate must precede docker compose build");
    }
}
