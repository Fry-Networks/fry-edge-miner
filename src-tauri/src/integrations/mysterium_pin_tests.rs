//! B12 — MystNodes `os error 740` and the permanently broken install.
//!
//! Two independent things made a broken MystNodes install permanent: any file
//! named `sdk_client.exe` at the partner path was trusted forever (no digest,
//! no size, no PE check), and `installed_version()` reported "installed" on
//! mere existence, so `toggle_integration` never called `install()` again —
//! while the app's only repair command is hard-gated to Olostep.

use super::*;

/// Strip line comments so a source-scanning assertion can never be satisfied
/// by prose. Same shape as `security_setup.rs`'s elevation-hygiene tests.
///
/// NOTE: this also truncates any line containing `//` inside a string — a URL,
/// for instance. Assertions about URLs therefore run against the RAW source
/// below, which is the stricter check anyway: the retired org must not appear
/// in a comment either.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

const MYSTERIUM_SRC: &str = include_str!("mysterium.rs");

#[test]
fn a_staged_binary_whose_digest_does_not_match_the_pin_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sdk_client.exe");
    std::fs::write(&path, vec![0x4du8; 200]).unwrap();

    assert!(
        !MysteriumIntegration::staged_binary_matches_pin(&path, SDK_CLIENT_SHA256),
        "200 junk bytes must never pass for a 16 MB partner binary"
    );

    // sha256 of the empty string, so the positive case is pinned too.
    std::fs::write(&path, b"").unwrap();
    assert!(MysteriumIntegration::staged_binary_matches_pin(
        &path,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    ));
}

#[test]
fn an_unreadable_binary_is_never_trusted() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist.exe");
    assert!(
        !MysteriumIntegration::staged_binary_matches_pin(&missing, SDK_CLIENT_SHA256),
        "a file that cannot even be read must not pass the gate"
    );
}

/// The pin has to be a real sha256, not a placeholder.
#[test]
fn the_pin_is_a_full_sha256() {
    assert_eq!(SDK_CLIENT_SHA256.len(), 64, "{SDK_CLIENT_SHA256}");
    assert!(SDK_CLIENT_SHA256.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(
        SDK_CLIENT_SHA256.chars().any(|c| c != '0'),
        "an all-zero pin is a placeholder, not a pin"
    );
}

/// §3 fence: `github.com/Fry-Foundation/*` is an operator-claimed defensive
/// org that must not be referenced at all, and `releases/latest` meant the
/// installed binary changed the moment a new release appeared.
#[test]
fn the_install_target_is_pinned_and_does_not_reference_the_retired_org() {
    // RAW, not code_only: `code_only` truncates at `//` and would silently
    // delete every URL, which would make both of these pass on the very
    // version they are meant to catch.
    assert!(
        !MYSTERIUM_SRC.contains("Fry-Foundation"),
        "still references the retired defensive org"
    );
    // A tag-pinned asset URL must exist...
    assert!(
        MYSTERIUM_SRC.contains("/releases/download/v"),
        "there is no tag-pinned asset URL"
    );
    // ...and install() must be the thing that uses it, rather than resolving
    // `latest` at install time. (The superseded lookup is left on disk
    // deliberately; what matters is that nothing calls it.)
    let code = code_only(MYSTERIUM_SRC);
    assert!(
        code.contains("download_file_with_options(SDK_CLIENT_URL"),
        "install() does not download the pinned asset"
    );
    let unpinned_call = format!("Self::fetch_latest{}().await?", "_release");
    assert!(
        !code.contains(&unpinned_call),
        "install() still resolves `latest` at install time"
    );
    assert!(
        code.contains("SDK_CLIENT_SHA256"),
        "install() is not digest-gated"
    );
}

/// The exact pre-fix shape: `install()` returning early on mere existence.
#[test]
fn install_no_longer_trusts_a_binary_for_merely_existing() {
    let code = code_only(MYSTERIUM_SRC);
    assert!(
        code.contains("staged_binary_matches_pin(&binary, SDK_CLIENT_SHA256)"),
        "install() must gate on the digest, not on existence"
    );
    // The early return is still there, but it is now reached only AFTER the
    // digest matches — the log line says which, so this cannot pass on a
    // version that returns on existence alone.
    assert!(
        code.contains("already present and matches its pin"),
        "install()'s early return is not digest-gated"
    );
}

#[test]
fn a_log_error_is_surfaced_with_the_line_that_caused_it() {
    let reason =
        log_error_reason(&["2026-09-20T10:00:00Z ERR identity not registered".to_string()])
            .expect("an ERR line must be reported");

    assert!(
        reason.contains("identity not registered"),
        "the matched line must reach the card: {reason}"
    );
    assert!(reason.starts_with("Error detected in SDK client logs"));
}

#[test]
fn an_ftl_line_counts_and_an_info_line_does_not() {
    assert!(log_error_reason(&["2026-09-20T10:00:00Z FTL cannot bind".to_string()]).is_some());
    assert!(log_error_reason(&["2026-09-20T10:00:00Z INF all good".to_string()]).is_none());
    assert!(log_error_reason(&[]).is_none());
    // " ERR " is space-bounded on purpose: a word like TERROR must not match.
    assert!(log_error_reason(&["2026-09-20T10:00:00Z INF TERROR handled".to_string()]).is_none());
}

/// sdk_client's argv carries `--user.token`, and the surfaced line goes to the
/// card AND into the debug bundle.
#[test]
fn a_surfaced_log_line_has_its_secrets_scrubbed() {
    let reason = log_error_reason(&[
        "2026-09-20T10:00:00Z ERR start failed args=--user.token=deadbeefdeadbeef".to_string(),
    ])
    .expect("an ERR line must be reported");

    assert!(
        !reason.contains("deadbeefdeadbeef"),
        "a token must never reach the card: {reason}"
    );
}

/// A single pathological log line must not become the whole card.
#[test]
fn a_very_long_log_line_is_truncated() {
    let long = format!("2026-09-20T10:00:00Z ERR {}", "x".repeat(4000));
    let reason = log_error_reason(&[long]).unwrap();
    assert!(
        reason.chars().count() < 4000,
        "reason was {} chars",
        reason.chars().count()
    );
}

/// The old code threw the matched line away entirely.
#[test]
fn the_fixed_reason_string_is_gone_from_the_source() {
    let code = code_only(MYSTERIUM_SRC);
    let discarded = format!(
        "Unhealthy(\"Error detected in SDK client logs\".{}string())",
        "to_"
    );
    assert!(
        !code.contains(&discarded),
        "the matched log line is still being discarded"
    );
}
