//! Durable Pawns.app consent state (CLI Addendum §5.8).
//!
//! Consent used to live only in `PAWNS_USER_CONSENT`, an environment variable
//! the UI cannot set and the machine forgets on reboot — so a device owner had
//! no way to consent from the app, and a consent given once was not durable.
//! The consent log is now the record of truth: the last entry this device
//! wrote decides whether sharing may start.
//!
//! These tests drive the log through a temp directory rather than the real
//! `%APPDATA%/FryEdgeMiner/partners/pawns` so a test run can never grant or
//! revoke consent for the machine it runs on.

use super::*;
use std::path::Path;

/// One raw log line for `device_id`, as the agent-facing writer emits them.
fn line(action: &str, device_id: &str, happened_at: &str) -> String {
    format!(
        r#"{{"action":"{}","happened_at":"{}","device_id":"{}","device_name":"test","wording_version":"1","wording_language":"en","wording":"x","agent_image":"iproyal/pawns-cli:latest","fem_version":"0.0.0"}}"#,
        action, happened_at, device_id
    )
}

fn write_log(dir: &Path, lines: &[String]) -> std::path::PathBuf {
    let path = dir.join("consent-log.jsonl");
    let body = lines
        .iter()
        .map(|l| format!("{}\n", l))
        .collect::<String>();
    std::fs::write(&path, body).expect("test log should be writable");
    path
}

#[test]
fn a_recorded_consent_makes_sharing_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_log(&dir.path(), &[line("consent", "dev-a", "2026-08-19T10:00:00Z")]);

    let entry = last_consent_entry_in(&path, "dev-a").expect("the entry should be found");
    assert_eq!(entry.action, "consent");
    assert_eq!(entry.happened_at, "2026-08-19T10:00:00Z");
    assert!(consent_is_active(&path, "dev-a"));
}

#[test]
fn a_recorded_withdrawal_takes_consent_away_again() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_log(
        &dir.path(),
        &[
            line("consent", "dev-a", "2026-08-19T10:00:00Z"),
            line("withdrawal", "dev-a", "2026-08-19T11:00:00Z"),
        ],
    );

    let entry = last_consent_entry_in(&path, "dev-a").expect("the entry should be found");
    assert_eq!(entry.action, "withdrawal");
    assert!(!consent_is_active(&path, "dev-a"));
}

#[test]
fn a_device_that_never_consented_is_not_sharing() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("consent-log.jsonl");

    assert!(last_consent_entry_in(&missing, "dev-a").is_none());
    assert!(!consent_is_active(&missing, "dev-a"));
}

#[test]
fn the_last_entry_decides_after_consent_withdrawal_consent() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_log(
        &dir.path(),
        &[
            line("consent", "dev-a", "2026-08-19T10:00:00Z"),
            line("withdrawal", "dev-a", "2026-08-19T11:00:00Z"),
            line("consent", "dev-a", "2026-08-19T12:00:00Z"),
        ],
    );

    let entry = last_consent_entry_in(&path, "dev-a").expect("the entry should be found");
    assert_eq!(entry.action, "consent");
    assert_eq!(
        entry.happened_at, "2026-08-19T12:00:00Z",
        "the newest entry should be the one reported to the UI"
    );
    assert!(consent_is_active(&path, "dev-a"));
}

#[test]
fn a_malformed_line_is_skipped_instead_of_losing_the_whole_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_log(
        &dir.path(),
        &[
            line("consent", "dev-a", "2026-08-19T10:00:00Z"),
            "{ this is not json".to_string(),
            String::new(),
            "null".to_string(),
            r#"{"action":42,"device_id":"dev-a"}"#.to_string(),
        ],
    );

    // A truncated write at the end of the file must not revoke a consent the
    // owner actually gave.
    assert!(
        consent_is_active(&path, "dev-a"),
        "a corrupt trailing line silently withdrew consent"
    );
}

#[test]
fn another_devices_entry_does_not_decide_this_devices_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_log(
        &dir.path(),
        &[
            line("consent", "dev-a", "2026-08-19T10:00:00Z"),
            line("withdrawal", "dev-b", "2026-08-19T11:00:00Z"),
        ],
    );

    assert!(
        consent_is_active(&path, "dev-a"),
        "another device's withdrawal must not stop this device sharing"
    );
    assert!(
        !consent_is_active(&path, "dev-b"),
        "dev-b withdrew and must not be treated as consenting"
    );
    assert!(
        !consent_is_active(&path, "dev-never-seen"),
        "an unknown device has given no consent"
    );
}

#[test]
fn an_unreadable_log_is_treated_as_no_consent() {
    let dir = tempfile::tempdir().unwrap();
    // A directory where the log should be: read fails, and failing open is the
    // only safe direction for a consent gate.
    let path = dir.path().join("consent-log.jsonl");
    std::fs::create_dir(&path).unwrap();

    assert!(last_consent_entry_in(&path, "dev-a").is_none());
    assert!(!consent_is_active(&path, "dev-a"));
}

#[test]
fn a_written_consent_line_still_carries_every_field_the_addendum_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("consent-log.jsonl");

    PawnsIntegration::record_consent_event_at(&path, "consent");

    let body = std::fs::read_to_string(&path).expect("the record should have been written");
    let record: serde_json::Value =
        serde_json::from_str(body.lines().next().expect("one line")).expect("valid JSON");
    let obj = record.as_object().expect("a JSON object");

    for field in [
        "action",
        "happened_at",
        "device_id",
        "device_name",
        "wording_version",
        "wording_language",
        "wording",
        "agent_image",
        "fem_version",
        // Added after the first nine: the record has to say what was agreed to,
        // not only when. The original nine keep their names and meaning.
        "terms_url",
        "terms_version",
    ] {
        assert!(obj.contains_key(field), "field {} was dropped: {}", field, body);
    }
    assert_eq!(obj.len(), 11, "the record gained or lost a field: {}", body);

    // The action names are the audit vocabulary — renaming either one silently
    // invalidates every record already on disk.
    assert_eq!(record["action"], "consent");
    assert_eq!(record["wording"], consent_disclosure());
    assert_eq!(record["wording_version"], consent_wording_version());
    assert_eq!(record["device_id"], PawnsIntegration::device_id());
    assert_eq!(record["terms_url"], terms_url());
    assert_eq!(record["terms_version"], terms_version());
}

#[test]
fn a_record_written_before_the_terms_fields_existed_is_still_read() {
    // `line()` writes the original nine-field shape. Consent already on disk
    // from an earlier build must keep counting — extending the format cannot
    // retroactively un-consent anyone.
    let dir = tempfile::tempdir().unwrap();
    let path = write_log(&dir.path(), &[line("consent", "dev-a", "2026-08-19T10:00:00Z")]);

    assert!(consent_is_active(&path, "dev-a"));
}

#[test]
fn the_recorded_terms_point_at_the_published_addendum() {
    // The URL is what the dialog links and what the record cites; a typo here
    // means every consent record references a document nobody can open.
    assert_eq!(
        terms_url(),
        "https://cdn.pawns.app/documents/PawnsApp-CLI-Addendum.pdf"
    );
    assert_eq!(terms_version(), "cli-addendum-2026-08-10");
}

#[test]
fn a_start_under_an_existing_consent_does_not_record_a_second_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("consent-log.jsonl");
    let device = PawnsIntegration::device_id();

    // The owner consents once through the UI, then the agent starts twice.
    PawnsIntegration::record_consent_event_at(&path, "consent");
    PawnsIntegration::record_start_consent_at(&path, &device);
    PawnsIntegration::record_start_consent_at(&path, &device);

    let body = std::fs::read_to_string(&path).unwrap();
    let consents = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter(|l| {
            serde_json::from_str::<serde_json::Value>(l).unwrap()["action"] == "consent"
        })
        .count();
    assert_eq!(
        consents, 1,
        "starting re-recorded consent the owner only gave once: {}",
        body
    );
}

#[test]
fn a_start_with_no_record_yet_writes_the_first_one() {
    // The headless env override consents without ever touching the log, so the
    // first start is what makes that consent durable.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("consent-log.jsonl");
    let device = PawnsIntegration::device_id();

    PawnsIntegration::record_start_consent_at(&path, &device);

    assert!(consent_is_active(&path, &device));
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 1);
}

#[test]
fn a_start_after_a_withdrawal_records_the_new_consent() {
    // Withdrawn then re-enabled is a fresh consent decision and must appear as
    // its own entry, not be swallowed by the de-duplication above.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("consent-log.jsonl");
    let device = PawnsIntegration::device_id();

    PawnsIntegration::record_consent_event_at(&path, "consent");
    PawnsIntegration::record_consent_event_at(&path, "withdrawal");
    PawnsIntegration::record_start_consent_at(&path, &device);

    assert!(consent_is_active(&path, &device));
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 3);
}

#[test]
fn a_withdrawal_is_written_under_the_exact_withdrawal_action_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("consent-log.jsonl");

    PawnsIntegration::record_consent_event_at(&path, "consent");
    PawnsIntegration::record_consent_event_at(&path, "withdrawal");

    let body = std::fs::read_to_string(&path).unwrap();
    let actions: Vec<String> = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).unwrap()["action"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(actions, vec!["consent", "withdrawal"]);

    // Appending, never rewriting: the earlier record is still there.
    assert!(!consent_is_active(&path, &PawnsIntegration::device_id()));
}

#[test]
fn recording_creates_the_partner_directory_when_it_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("consent-log.jsonl");

    PawnsIntegration::record_consent_event_at(&path, "consent");

    assert!(path.exists(), "the consent record must survive a first run");
    assert!(consent_is_active(&path, &PawnsIntegration::device_id()));
}

#[test]
fn the_env_override_still_grants_consent_for_headless_runs() {
    // CI and headless installs have no UI to click, so the documented
    // PAWNS_USER_CONSENT escape hatch has to keep working.
    assert!(consent_from_env_value(Some("accepted")));
    assert!(consent_from_env_value(Some("ACCEPTED")));
    assert!(consent_from_env_value(Some(" accepted ")));
    assert!(!consent_from_env_value(Some("no")));
    assert!(!consent_from_env_value(Some("")));
    assert!(!consent_from_env_value(None));
}
