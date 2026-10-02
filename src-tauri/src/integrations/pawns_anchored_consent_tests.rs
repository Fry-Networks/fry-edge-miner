//! FAIL-3 (pre-tag review of 0.4.34): the Pawns consent decision was split
//! across two logs and merged by wall-clock timestamp. The anchored log takes
//! every grant, withdrawal and revoke; the storage-root log still took the
//! start-consent mirror. A withdrawal recorded in the anchored log could then be
//! outranked by a consent in the storage-root log that was stamped later (clock
//! skew) or in the same second (ties went to the last path): the gate failed
//! OPEN after the owner withdrew. These tests drive the real resolver on temp
//! logs only, never the host's real consent log.

use super::last_consent_entry_across;
use std::path::{Path, PathBuf};

const DEVICE: &str = "fail3-test-device";

fn write_log(path: &Path, entries: &[(&str, &str)]) {
    let body: String = entries
        .iter()
        .map(|(action, at)| {
            serde_json::json!({ "action": action, "happened_at": at, "device_id": DEVICE })
                .to_string()
                + "\n"
        })
        .collect();
    std::fs::write(path, body).expect("temp log write");
}

/// Resolve across [anchored, storage-root] exactly as `consent_record()` orders them.
fn resolve(anchored: &[(&str, &str)], storage_root: &[(&str, &str)]) -> Option<String> {
    let dir = tempfile::tempdir().expect("tempdir");
    let a = dir.path().join("anchored.jsonl");
    let s = dir.path().join("storage-root.jsonl");
    write_log(&a, anchored);
    write_log(&s, storage_root);
    let paths: Vec<PathBuf> = vec![a, s];
    last_consent_entry_across(&paths, DEVICE).map(|r| r.action)
}

#[test]
fn a_withdrawal_in_the_anchored_log_is_final_against_a_later_stamped_consent() {
    let got = resolve(
        &[
            ("consent", "2026-09-24T10:00:00Z"),
            ("withdrawal", "2026-09-24T11:00:00Z"),
        ],
        &[("consent", "2026-09-24T12:00:00Z")],
    );
    assert_eq!(
        got.as_deref(),
        Some("withdrawal"),
        "FAIL-3: a consent stamped later in the storage-root log outranked the owner's \
         recorded withdrawal (fail-open)"
    );
}

#[test]
fn a_same_second_tie_never_resolves_to_consent_over_a_withdrawal() {
    let got = resolve(
        &[("withdrawal", "2026-09-24T11:00:00Z")],
        &[("consent", "2026-09-24T11:00:00Z")],
    );
    assert_eq!(
        got.as_deref(),
        Some("withdrawal"),
        "FAIL-3: a same-second tie went to the storage-root consent (fail-open)"
    );
}

/// Any decision in the anchored log that is not a consent is final: fail
/// closed on a vocabulary the resolver does not know, too.
#[test]
fn any_non_consent_decision_in_the_anchored_log_is_final() {
    let got = resolve(
        &[
            ("consent", "2026-09-24T10:00:00Z"),
            ("revoked", "2026-09-24T11:00:00Z"),
        ],
        &[("consent", "2026-09-24T12:00:00Z")],
    );
    assert_ne!(
        got.as_deref(),
        Some("consent"),
        "FAIL-3: an unrecognised anchored decision fell through to a storage-root consent"
    );
}

/// SUPPORTING (green before and after): the direction the RC already got
/// right stays right — a newer storage-root withdrawal beats an anchored consent.
#[test]
fn a_newer_withdrawal_anywhere_still_beats_an_anchored_consent() {
    let got = resolve(
        &[("consent", "2026-09-24T10:00:00Z")],
        &[("withdrawal", "2026-09-24T11:00:00Z")],
    );
    assert_eq!(got.as_deref(), Some("withdrawal"));
    // Control: with no withdrawal anywhere, a consent is found.
    let got = resolve(&[("consent", "2026-09-24T10:00:00Z")], &[]);
    assert_eq!(
        got.as_deref(),
        Some("consent"),
        "control: the resolver reads the logs"
    );
}

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The start-consent mirror is what split the logs. It must go to the anchored
/// log like every other decision, and only when the headless override is what
/// authorised the start and nothing on record already covers it. `start()`
/// resolves the real paths, so this reads the real source.
#[test]
fn the_start_consent_record_goes_to_the_anchored_log_only_for_the_override() {
    let code = code_only(include_str!("pawns.rs"));
    let at = code
        .find("fn record_start_consent()")
        .expect("record_start_consent must exist");
    let body = &code[at..at + code[at..].find("\n    }\n").expect("fn must close")];
    // Control: the other writer already targets the anchored log.
    let ev = code
        .find("fn record_consent_event(")
        .expect("control: record_consent_event must exist");
    let ev_body = &code[ev..ev + code[ev..].find("\n    }\n").unwrap()];
    assert!(
        ev_body.contains("stable_consent_log()"),
        "control: grants and withdrawals must go to the anchored log"
    );

    assert!(
        body.contains("stable_consent_log()") && !body.contains("Self::consent_log()"),
        "FAIL-3: the start-consent mirror is written to the storage-root log, splitting \
         the consent decision across two logs:\n{body}"
    );
    assert!(
        body.contains("consent_active()") && body.contains("PAWNS_USER_CONSENT"),
        "FAIL-3: the start-consent record must be deduplicated against the same \
         resolution the gate uses and written only for the headless override:\n{body}"
    );
}
