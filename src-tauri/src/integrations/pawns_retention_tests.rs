//! Consent-log retention (24 months) with archival.
//!
//! The log is append-only and never pruned, so a long-lived install grows it
//! forever. Entries older than the retention window move to a sibling archive
//! file rather than being deleted: they are the durable record the CLI Addendum
//! (§5.8) asks for, and the machine that holds them is the only place they
//! exist. Two entries are never moved — the newest entry for each device (it is
//! the consent state the gate reads) and any line the reader cannot parse (a
//! half-written trailing line must not be quietly discarded).
//!
//! Like the other consent tests these run entirely in a temp directory, so a
//! test run can never rotate the consent log of the machine it runs on.

// clippy::identity_op — the fixture tables below write ages as `N * DAY`
// (`800 * DAY`, `1100 * DAY`, `700 * DAY`, `5 * DAY`). Collapsing the `1 * DAY`
// cases to a bare `DAY` is what clippy suggests and is behaviour-preserving, but
// it breaks the column alignment that makes those tables readable at a glance.
// The parallelism is deliberate, so the lint is silenced rather than obeyed.
#![allow(clippy::identity_op)]

use super::*;
use std::path::Path;

/// Seconds per day, for building fixture ages.
const DAY: i64 = 86_400;

/// A fixed "now" so the fixtures do not drift with the wall clock:
/// 2026-09-08T00:00:00Z.
const NOW: i64 = 1_788_912_000;

fn stamp(secs_ago: i64) -> String {
    let secs = NOW - secs_ago;
    let days = secs.div_euclid(DAY);
    let tod = secs.rem_euclid(DAY);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m,
        d,
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

fn entry(action: &str, device_id: &str, secs_ago: i64) -> String {
    format!(
        r#"{{"action":"{}","happened_at":"{}","device_id":"{}","device_name":"test","wording_version":"1","wording_language":"en","wording":"x","agent_image":"iproyal/pawns-cli:latest","fem_version":"0.0.0","terms_url":"u","terms_version":"1"}}"#,
        action,
        stamp(secs_ago),
        device_id
    )
}

fn write_lines(path: &Path, lines: &[String]) {
    let body = lines.iter().map(|l| format!("{}\n", l)).collect::<String>();
    std::fs::write(path, body).expect("fixture log should be writable");
}

fn read_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|b| b.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default()
}

fn paths(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        dir.join("consent-log.jsonl"),
        dir.join("consent-log-archive.jsonl"),
    )
}

#[test]
fn an_old_entry_is_archived_once_the_device_has_a_newer_one() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (log, archive) = paths(dir.path());
    let old = entry("consent", "dev-a", 800 * DAY);
    let recent = entry("withdrawal", "dev-a", 10 * DAY);
    write_lines(&log, &[old.clone(), recent.clone()]);

    let summary = PawnsIntegration::rotate_consent_log_at(&log, &archive, NOW);

    assert_eq!(summary.before, 2, "two entries went in");
    assert_eq!(
        summary.retained, 1,
        "only the recent one stays: {summary:?}"
    );
    assert_eq!(
        summary.archived, 1,
        "the 800-day-old one is archived: {summary:?}"
    );
    assert_eq!(
        read_lines(&log),
        vec![recent],
        "the active log keeps the newest entry"
    );
    assert_eq!(
        read_lines(&archive),
        vec![old],
        "the trimmed entry is in the archive, not gone"
    );
}

#[test]
fn an_old_entry_that_is_a_devices_only_record_is_kept() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (log, archive) = paths(dir.path());
    // Consent given three years ago and never revisited: it is still this
    // device's consent state, so rotating it away would silently revoke sharing.
    let ancient = entry("consent", "dev-quiet", 1100 * DAY);
    write_lines(&log, std::slice::from_ref(&ancient));
    assert!(
        consent_is_active(&log, "dev-quiet"),
        "precondition: the ancient consent is the active state"
    );

    let summary = PawnsIntegration::rotate_consent_log_at(&log, &archive, NOW);

    assert_eq!(
        summary.archived, 0,
        "a device's only record is never archived: {summary:?}"
    );
    assert_eq!(read_lines(&log), vec![ancient]);
    assert!(
        !archive.exists(),
        "nothing to archive means no archive file is created"
    );
    assert!(
        consent_is_active(&log, "dev-quiet"),
        "consent state survives rotation"
    );
}

#[test]
fn entries_inside_the_window_are_all_kept() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (log, archive) = paths(dir.path());
    let lines = vec![
        entry("consent", "dev-a", 700 * DAY),
        entry("withdrawal", "dev-a", 400 * DAY),
        entry("consent", "dev-a", 5 * DAY),
    ];
    write_lines(&log, &lines);

    let summary = PawnsIntegration::rotate_consent_log_at(&log, &archive, NOW);

    assert_eq!(
        summary.archived, 0,
        "729 days is inside a 730-day window: {summary:?}"
    );
    assert_eq!(read_lines(&log), lines);
}

#[test]
fn a_malformed_line_is_kept_rather_than_archived() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (log, archive) = paths(dir.path());
    let garbage = "{not json".to_string();
    let old = entry("consent", "dev-a", 900 * DAY);
    let recent = entry("consent", "dev-a", 1 * DAY);
    write_lines(&log, &[garbage.clone(), old.clone(), recent.clone()]);

    let summary = PawnsIntegration::rotate_consent_log_at(&log, &archive, NOW);

    let kept = read_lines(&log);
    assert!(
        kept.contains(&garbage),
        "an unreadable line is never discarded: {kept:?}"
    );
    assert_eq!(
        kept,
        vec![garbage, recent],
        "only the superseded old entry moves"
    );
    assert_eq!(read_lines(&archive), vec![old]);
    assert_eq!(summary.archived, 1);
}

#[test]
fn an_entry_with_an_unparseable_timestamp_is_kept() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (log, archive) = paths(dir.path());
    let undated =
        r#"{"action":"consent","happened_at":"whenever","device_id":"dev-a"}"#.to_string();
    let recent = entry("consent", "dev-a", 2 * DAY);
    write_lines(&log, &[undated.clone(), recent.clone()]);

    let summary = PawnsIntegration::rotate_consent_log_at(&log, &archive, NOW);

    assert_eq!(summary.archived, 0, "age unknown means keep: {summary:?}");
    assert_eq!(read_lines(&log), vec![undated, recent]);
}

#[test]
fn every_devices_state_reads_the_same_after_rotation() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (log, archive) = paths(dir.path());
    write_lines(
        &log,
        &[
            entry("consent", "dev-a", 900 * DAY),
            entry("withdrawal", "dev-a", 800 * DAY),
            entry("consent", "dev-a", 3 * DAY),
            entry("consent", "dev-b", 1000 * DAY),
            entry("consent", "dev-c", 950 * DAY),
            entry("withdrawal", "dev-c", 940 * DAY),
        ],
    );
    let before: Vec<bool> = ["dev-a", "dev-b", "dev-c"]
        .iter()
        .map(|d| consent_is_active(&log, d))
        .collect();
    assert_eq!(before, vec![true, true, false], "precondition");

    PawnsIntegration::rotate_consent_log_at(&log, &archive, NOW);

    let after: Vec<bool> = ["dev-a", "dev-b", "dev-c"]
        .iter()
        .map(|d| consent_is_active(&log, d))
        .collect();
    assert_eq!(
        after, before,
        "rotation must not change any device's answer"
    );
}

#[test]
fn a_second_rotation_changes_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (log, archive) = paths(dir.path());
    write_lines(
        &log,
        &[
            entry("consent", "dev-a", 900 * DAY),
            entry("consent", "dev-a", 4 * DAY),
        ],
    );

    let first = PawnsIntegration::rotate_consent_log_at(&log, &archive, NOW);
    let log_after_first = read_lines(&log);
    let archive_after_first = read_lines(&archive);
    let second = PawnsIntegration::rotate_consent_log_at(&log, &archive, NOW);

    assert_eq!(first.archived, 1);
    assert_eq!(second.archived, 0, "idempotent: {second:?}");
    assert_eq!(read_lines(&log), log_after_first);
    assert_eq!(
        read_lines(&archive),
        archive_after_first,
        "a repeat run must not append the same entry to the archive twice"
    );
}

#[test]
fn archived_entries_append_to_an_existing_archive_in_order() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (log, archive) = paths(dir.path());
    let already = entry("consent", "dev-old", 2000 * DAY);
    write_lines(&archive, std::slice::from_ref(&already));
    let first = entry("consent", "dev-a", 900 * DAY);
    let second = entry("withdrawal", "dev-a", 850 * DAY);
    write_lines(
        &log,
        &[
            first.clone(),
            second.clone(),
            entry("consent", "dev-a", 1 * DAY),
        ],
    );

    PawnsIntegration::rotate_consent_log_at(&log, &archive, NOW);

    assert_eq!(
        read_lines(&archive),
        vec![already, first, second],
        "existing archive content is preserved and new entries append in log order"
    );
}

#[test]
fn a_missing_log_is_not_an_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (log, archive) = paths(dir.path());

    let summary = PawnsIntegration::rotate_consent_log_at(&log, &archive, NOW);

    assert_eq!(
        (summary.before, summary.retained, summary.archived),
        (0, 0, 0)
    );
    assert!(
        !log.exists() && !archive.exists(),
        "rotation creates nothing when there is no log"
    );
}
