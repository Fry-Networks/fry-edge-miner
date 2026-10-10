//! C9: the Diiisco bearer token is never compiled in; it comes from the
//! hardwareapi credential response. Synthetic values only.
use crate::api::types::CredentialInfo;

fn manifest(rel: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// (a) No compile-time read of the token anywhere in the Diiisco source or build.rs.
#[test]
fn c9_no_compile_time_bearer_token() {
    let src = manifest("src/integrations/diiisco.rs");
    let build = manifest("build.rs");
    assert!(
        !src.contains("env!(\"DIIISCO_BEARER_TOKEN\")"),
        "diiisco.rs still reads DIIISCO_BEARER_TOKEN at compile time"
    );
    assert!(
        !build.contains("env!(\"DIIISCO_BEARER_TOKEN\")"),
        "build.rs reads DIIISCO_BEARER_TOKEN at compile time"
    );
    assert!(
        !build.contains("rustc-env=DIIISCO"),
        "build.rs exports a DIIISCO value to rustc"
    );
}

/// (b) The credential response carries the field and it round-trips.
#[test]
fn c9_credential_info_round_trips_diiisco_bearer_token() {
    let body = r#"{"miner_key":"FEM-C9TEST","diiisco_bearer_token":"c9-synthetic-diiisco"}"#;
    let info: CredentialInfo = serde_json::from_str(body).expect("deserialises");
    let v = serde_json::to_value(&info).expect("serialises");
    assert_eq!(
        v.get("diiisco_bearer_token").and_then(|x| x.as_str()),
        Some("c9-synthetic-diiisco"),
        "diiisco_bearer_token not carried by CredentialInfo: {v}"
    );
}
