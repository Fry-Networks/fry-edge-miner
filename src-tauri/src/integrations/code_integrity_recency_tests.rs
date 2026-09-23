//! G4 findings 6 and 16 — a stale code-integrity event permanently disabled
//! recovery.
//!
//! The CodeIntegrity/Operational channel is near-silent on a normal machine and
//! the query had no time filter, so ONE historical block survived in the last
//! 20 events indefinitely. A user who followed FEM's own instruction, allowed
//! the file and ran for weeks would, on the next ORDINARY death, have that
//! months-old event re-read, the "Awaiting administrator action" marker
//! returned, and `recovery_action` suppressed forever — with the card showing
//! instructions for a problem they had already fixed.

use super::*;

fn event(name: &str, id: &str, at: chrono::DateTime<chrono::Utc>) -> String {
    format!(
        "<Event><System><EventID>{id}</EventID>\
         <TimeCreated SystemTime='{}'/></System>\
         <EventData><Data>\\Device\\HarddiskVolume3\\Users\\fry\\{name}</Data></EventData></Event>",
        at.to_rfc3339()
    )
}

fn titan_dll() -> std::path::PathBuf {
    std::path::PathBuf::from(
        r"C:\Users\fry\AppData\Local\FryEdgeMiner\partners\titan\goworkerd.dll",
    )
}

/// The reported sequence: blocked on day 1, user allows the file, titan runs
/// for weeks, then dies for an ordinary reason.
#[test]
fn a_month_old_block_no_longer_suppresses_recovery() {
    let now = chrono::Utc::now();
    let listing = event("goworkerd.dll", "3033", now - chrono::Duration::days(30));

    assert!(
        first_block_for(&listing, &titan_dll()).is_some(),
        "characterization: the name-only matcher still finds the stale event"
    );
    assert!(
        first_block_for_at(&listing, &titan_dll(), now, BLOCK_RECENCY).is_none(),
        "a month-old block must not describe the current state"
    );
}

/// A block that just happened must still be detected — the whole point.
#[test]
fn a_block_that_just_happened_is_still_detected() {
    let now = chrono::Utc::now();
    let listing = event("goworkerd.dll", "3033", now - chrono::Duration::seconds(5));

    let found = first_block_for_at(&listing, &titan_dll(), now, BLOCK_RECENCY)
        .expect("a fresh block must be detected");
    assert!(found.contains("3033"));
}

#[test]
fn the_window_boundary_is_inclusive_and_bounded() {
    let now = chrono::Utc::now();
    let inside = event(
        "goworkerd.dll",
        "3033",
        now - chrono::Duration::from_std(BLOCK_RECENCY).unwrap(),
    );
    let outside = event(
        "goworkerd.dll",
        "3033",
        now - chrono::Duration::from_std(BLOCK_RECENCY).unwrap() - chrono::Duration::seconds(1),
    );

    assert!(event_is_recent(&inside, now, BLOCK_RECENCY));
    assert!(!event_is_recent(&outside, now, BLOCK_RECENCY));
}

/// Fails CLOSED, unlike titan's log-line recency. An unreadable timestamp here
/// would SUPPRESS recovery, and suppression is the dangerous direction.
#[test]
fn an_unreadable_timestamp_is_not_treated_as_recent() {
    let now = chrono::Utc::now();

    assert!(!event_is_recent(
        "<Event><System></System></Event>",
        now,
        BLOCK_RECENCY
    ));
    assert!(!event_is_recent(
        "<Event><TimeCreated SystemTime='not-a-date'/></Event>",
        now,
        BLOCK_RECENCY
    ));
    assert_eq!(event_time("<Event/>"), None);
}

/// A clock skew that puts an event in the future must not count as recent
/// either — that is the other way to accidentally suppress recovery forever.
#[test]
fn an_event_from_the_future_is_not_recent() {
    let now = chrono::Utc::now();
    let listing = event("goworkerd.dll", "3033", now + chrono::Duration::days(1));

    assert!(!event_is_recent(&listing, now, BLOCK_RECENCY));
}

#[test]
fn the_timestamp_is_read_from_either_quoting_style() {
    let at = chrono::Utc::now();
    let single = format!("<TimeCreated SystemTime='{}'/>", at.to_rfc3339());
    let double = format!("<TimeCreated SystemTime=\"{}\"/>", at.to_rfc3339());

    assert!(event_time(&single).is_some());
    assert!(event_time(&double).is_some());
}

/// The query must bound itself at the source too, so a near-silent channel
/// cannot hand back months of history for the Rust side to filter.
#[test]
fn the_query_bounds_itself_by_time() {
    let script = recent_blocks_script();
    assert!(script.contains("StartTime="), "{script}");
    assert!(
        script.contains(&BLOCK_RECENCY.as_secs().to_string()),
        "the query window must match the one the Rust side enforces: {script}"
    );
}

/// A block naming a DIFFERENT image must never be attributed to ours.
#[test]
fn another_images_block_is_not_ours() {
    let now = chrono::Utc::now();
    let listing = event("some-other-thing.dll", "3033", now);

    assert!(first_block_for_at(&listing, &titan_dll(), now, BLOCK_RECENCY).is_none());
}
