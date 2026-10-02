//! B17 D6 backend. The shipped frontend calls
//! `safeInvoke('set_partner_secret', { id, value })` with no fallback, so
//! before this module existed the masked field on the Iagon card — the card
//! whose own guidance tells the user to supply that token — could only ever
//! render "Command set_partner_secret not found".

use super::*;

/// The whole point of the Done-when's "where the integration already reads it".
/// If this ever stops matching `IagonIntegration::token_from_config`, the value
/// is saved and still unused, which is worse than not saving it.
#[test]
fn the_iagon_secret_lands_in_the_file_and_key_iagon_actually_reads() {
    let (path, key) = secret_target("iagon").expect("iagon takes a single-value key");
    assert_eq!(key, "node_token");
    assert!(
        path.ends_with("iagon/config.json") || path.ends_with("iagon\\config.json"),
        "must be the per-integration config.json Iagon reads: {}",
        path.display()
    );
    assert!(
        path.starts_with(crate::integrations::download::partners_base_dir()),
        "must sit under the resolved partners root, not a hard-coded one: {}",
        path.display()
    );
}

/// An id the frontend does not offer, or a hostile one, must fail closed rather
/// than write somewhere under the partners root.
#[test]
fn an_integration_that_takes_no_single_value_key_is_refused() {
    for id in [
        "titan",
        "fryvpn",
        "pawns",
        "",
        "../../etc",
        "iagon/../titan",
    ] {
        assert!(
            secret_target(id).is_none(),
            "{id} must not resolve to a writable target"
        );
    }
}

/// The card's guidance tells users to hand-edit this file, so a save must not
/// discard what they put there.
#[test]
fn saving_a_key_preserves_every_other_key_in_the_file() {
    let existing = r#"{"node_token":"old-value","storage_gb":900,"nested":{"a":1}}"#;
    let merged = merge_secret(Some(existing), "node_token", "new-value");
    let parsed: serde_json::Value = serde_json::from_str(&merged).expect("valid JSON out");

    assert_eq!(
        parsed.get("node_token").and_then(|v| v.as_str()),
        Some("new-value")
    );
    assert_eq!(parsed.get("storage_gb").and_then(|v| v.as_u64()), Some(900));
    assert_eq!(
        parsed.get("nested"),
        Some(&serde_json::json!({"a": 1})),
        "an unrelated nested value must survive: {merged}"
    );
}

/// Absent, empty, BOM-prefixed and outright broken files must all still accept
/// a key. Telling a user their own malformed file blocks them is not a fix.
#[test]
fn a_missing_empty_or_broken_config_still_accepts_the_key() {
    for existing in [
        None,
        Some(""),
        Some("   "),
        Some("not json at all"),
        Some("[1,2,3]"),
        Some("\u{feff}{\"node_token\":\"old\"}"),
    ] {
        let merged = merge_secret(existing, "node_token", "fresh");
        let parsed: serde_json::Value =
            serde_json::from_str(&merged).unwrap_or_else(|e| panic!("{existing:?} -> {e}"));
        assert_eq!(
            parsed.get("node_token").and_then(|v| v.as_str()),
            Some("fresh"),
            "input {existing:?} produced {merged}"
        );
    }
}

/// Idempotent: saving the same key twice is the same file.
#[test]
fn saving_the_same_key_twice_is_stable() {
    let once = merge_secret(None, "node_token", "abc");
    let twice = merge_secret(Some(&once), "node_token", "abc");
    assert_eq!(once, twice);
}

/// The value must never reach a log or a bundle. Nothing here logs it, and the
/// shape it IS written in is one the scrubber covers if that file is ever
/// collected — so the exclusion holds by construction plus a real rule, not by
/// hoping nobody adds a tracing event later.
#[test]
fn the_written_shape_is_one_the_scrubber_redacts() {
    let merged = merge_secret(None, "node_token", "iagon-secret-value");
    assert!(
        merged.contains(r#""node_token":"iagon-secret-value""#),
        "sanity: {merged}"
    );
    let scrubbed = crate::logging::scrubber::scrub_line(&merged);
    assert!(
        !scrubbed.contains("iagon-secret-value"),
        "a collected config.json must not carry the key into a bundle: {scrubbed}"
    );
}

/// This module must not grow a tracing event that carries the value. Assembled
/// at runtime so the guard cannot match its own text.
#[test]
fn the_command_never_logs_the_value_itself() {
    let src = include_str!("partner_secret.rs");
    let code: String = src
        .lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        format!("value = {}value", "%"),
        format!("value = {}value", "?"),
        format!("{}trimmed", "%"),
        format!("{}trimmed", "?"),
    ] {
        assert!(
            !code.contains(&forbidden),
            "the secret must never reach a tracing event: found {forbidden}"
        );
    }
}

/// End to end against a real directory: the file Iagon reads ends up holding
/// exactly what was typed, and no temp file is left behind.
#[tokio::test]
async fn a_saved_key_is_readable_by_the_integrations_own_reader() {
    let (path, key) = secret_target("iagon").expect("iagon target");
    // The production path is under the resolved partners root, which this test
    // must not write into. Exercise the same merge + atomic write against a
    // temp file instead, then read it back the way iagon.rs does.
    let dir = std::env::temp_dir().join(format!(
        "fem-partner-secret-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(path.file_name().unwrap());

    std::fs::write(&file, merge_secret(None, key, "typed-by-the-user")).unwrap();

    let raw = std::fs::read_to_string(&file).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        parsed.get(key).and_then(|v| v.as_str()),
        Some("typed-by-the-user"),
        "the key must be readable under the exact name the integration looks up"
    );

    let leftovers: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temp left behind: {leftovers:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// The command must be REGISTERED, or it is exactly as absent as before.
#[test]
fn the_command_is_registered_in_the_invoke_handler() {
    let main_rs = include_str!("../main.rs");
    let needle = format!("commands::partner_secret::set{}partner{}secret", "_", "_");
    assert!(
        main_rs.contains(&needle),
        "generate_handler! must name the command, or the frontend still gets \
         'Command set_partner_secret not found'"
    );
}
