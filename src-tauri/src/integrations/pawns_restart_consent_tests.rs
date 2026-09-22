//! B18 — "Pawns consent resets every 1–2 h".
//!
//! Two independent ways a consent nobody withdrew disappeared.
//!
//! The supervisor recycles a container after a crash, a Docker outage or a
//! startup timeout by calling `stop()` — which appended a `withdrawal` to the
//! durable §5.8 log. That permanently revoked the owner's consent, and because
//! "needs your consent" is exempt from recovery the integration then sat in
//! that state with `enabled` still true and no automatic way back. A Docker
//! outage was the worst case: the same outage both triggered the restart and
//! guaranteed the consent loss, because the withdrawal was written even on the
//! error arm.
//!
//! Separately, the log hung off the CONFIGURABLE storage root, so pointing
//! storage elsewhere — or having a configured root silently fall back because
//! it was unwritable — made FEM read a different file and report the same
//! thing with no restart involved at all.
//!
//! These drive temp directories, never the real
//! `%APPDATA%/FryEdgeMiner/partners/pawns`, so a test run can neither grant
//! nor revoke consent for the machine it runs on. The helpers are copied
//! rather than imported so `pawns_consent_tests.rs` stays untouched.

use super::*;
use std::path::Path;

fn line(action: &str, device_id: &str, happened_at: &str) -> String {
    format!(
        r#"{{"action":"{}","happened_at":"{}","device_id":"{}","device_name":"test","wording_version":"1","wording_language":"en","wording":"x","agent_image":"iproyal/pawns-cli:latest","fem_version":"0.0.0"}}"#,
        action, happened_at, device_id
    )
}

fn write_log(dir: &Path, lines: &[String]) -> PathBuf {
    let path = dir.join("consent-log.jsonl");
    let body = lines.iter().map(|l| format!("{}\n", l)).collect::<String>();
    std::fs::write(&path, body).expect("test log should be writable");
    path
}

#[test]
fn stop_records_a_withdrawal_only_for_a_user_disable() {
    assert!(stop_records_withdrawal(StopReason::UserDisable));
    assert!(!stop_records_withdrawal(StopReason::SupervisorRestart));
}

/// The consent-effect half of the stop path, for a restart: the log must come
/// back byte-identical and consent must still be active.
#[test]
fn a_supervisor_restart_leaves_a_recorded_consent_intact() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_log(
        dir.path(),
        &[line("consent", "dev-a", "2026-09-20T10:00:00Z")],
    );
    let before = std::fs::read(&path).unwrap();

    if stop_records_withdrawal(StopReason::SupervisorRestart) {
        PawnsIntegration::record_consent_event_at(&path, "withdrawal");
    }

    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "a restart must not write to the durable consent record at all"
    );
    assert!(
        consent_is_active(&path, "dev-a"),
        "the owner never withdrew; consent must survive the recycle"
    );
}

/// The §5.8 requirement is untouched for the case that really is a withdrawal.
#[test]
fn a_user_initiated_disable_still_records_the_withdrawal() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_log(
        dir.path(),
        &[line("consent", "dev-a", "2026-09-20T10:00:00Z")],
    );

    if stop_records_withdrawal(StopReason::UserDisable) {
        PawnsIntegration::record_consent_event_at(&path, "withdrawal");
    }

    let body = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        body.lines().count(),
        2,
        "a real disable must append exactly one record: {body}"
    );
    assert!(body.contains(r#""action":"withdrawal""#), "{body}");
}

/// An install that consented under a custom storage root must keep its
/// consent once the record is anchored — nothing is moved or copied.
#[test]
fn a_consent_recorded_under_a_custom_root_is_still_seen_from_the_default_root() {
    let custom = tempfile::tempdir().unwrap();
    let stable = tempfile::tempdir().unwrap();
    let custom_log = write_log(
        custom.path(),
        &[line("consent", "dev-a", "2026-09-20T10:00:00Z")],
    );
    let stable_log = stable.path().join("consent-log.jsonl");

    let found = last_consent_entry_across(&[stable_log, custom_log], "dev-a")
        .expect("a consent recorded anywhere we look must be found");
    assert_eq!(found.action, "consent");
}

/// Fails CLOSED across roots, exactly as a single log does: a stale consent
/// must never outrank a newer withdrawal.
#[test]
fn the_newest_entry_wins_across_roots() {
    let custom = tempfile::tempdir().unwrap();
    let stable = tempfile::tempdir().unwrap();
    let custom_log = write_log(
        custom.path(),
        &[line("consent", "dev-a", "2026-09-20T10:00:00Z")],
    );
    let stable_log = write_log(
        stable.path(),
        &[line("withdrawal", "dev-a", "2026-09-21T10:00:00Z")],
    );

    let found = last_consent_entry_across(&[stable_log, custom_log], "dev-a").unwrap();
    assert_eq!(
        found.action, "withdrawal",
        "a newer withdrawal must win over an older consent in another root"
    );
}

/// And the other direction: a consent given AFTER a withdrawal wins too, so a
/// user who re-consents is not held to an older entry in a stale root.
#[test]
fn a_newer_consent_wins_over_an_older_withdrawal_in_another_root() {
    let custom = tempfile::tempdir().unwrap();
    let stable = tempfile::tempdir().unwrap();
    let custom_log = write_log(
        custom.path(),
        &[line("withdrawal", "dev-a", "2026-09-20T10:00:00Z")],
    );
    let stable_log = write_log(
        stable.path(),
        &[line("consent", "dev-a", "2026-09-21T10:00:00Z")],
    );

    assert_eq!(
        last_consent_entry_across(&[stable_log, custom_log], "dev-a")
            .unwrap()
            .action,
        "consent"
    );
}

#[test]
fn another_devices_consent_is_never_borrowed() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_log(
        dir.path(),
        &[line("consent", "dev-b", "2026-09-20T10:00:00Z")],
    );

    assert!(last_consent_entry_across(&[path], "dev-a").is_none());
}

#[test]
fn a_missing_log_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("nope").join("consent-log.jsonl");

    assert!(last_consent_entry_across(&[absent], "dev-a").is_none());
    assert!(last_consent_entry_across(&[], "dev-a").is_none());
}
