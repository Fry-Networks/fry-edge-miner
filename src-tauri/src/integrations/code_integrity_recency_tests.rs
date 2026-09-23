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
        first_block_for_ignoring_recency(&listing, &titan_dll()).is_some(),
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

/// FAILS OPEN on an unreadable timestamp.
///
/// This is a deliberate reversal of my first version. T2 pointed out that the
/// fixture this parser was written against is SYNTHETIC — never replaced by a
/// capture from a machine that actually refused a DLL. Failing closed would
/// therefore have silently disabled code-integrity detection ENTIRELY on any
/// host whose XML we read wrongly, restoring the respawn loop B15 defect 5
/// exists to stop. The query is already bounded by StartTime, so this check
/// only ever EXCLUDES an event it can positively prove is too old.
#[test]
fn an_unreadable_timestamp_does_not_disable_detection() {
    let now = chrono::Utc::now();

    assert!(event_is_recent(
        "<Event><System></System></Event>",
        now,
        BLOCK_RECENCY
    ));
    assert!(event_is_recent(
        "<Event><TimeCreated SystemTime='not-a-date'/></Event>",
        now,
        BLOCK_RECENCY
    ));
    assert_eq!(event_time("<Event/>"), None);
}

/// A block with no readable stamp must still reach the user, or the feature is
/// silently off.
#[test]
fn a_block_with_no_readable_stamp_is_still_detected() {
    let now = chrono::Utc::now();
    let listing = concat!(
        "<Event><System><EventID>3033</EventID></System>",
        "<EventData><Data>goworkerd.dll</Data></EventData></Event>"
    );

    assert!(
        first_block_for_at(listing, &titan_dll(), now, BLOCK_RECENCY).is_some(),
        "an unparseable stamp must not hide a real block"
    );
}

/// The one case the query cannot protect against: StartTime is a lower bound,
/// so a future-dated event is returned forever and would suppress recovery
/// forever. Ordinary jitter is tolerated; an absurd stamp is not.
#[test]
fn an_implausibly_future_event_is_not_recent_but_jitter_is_tolerated() {
    let now = chrono::Utc::now();

    let jitter = event("goworkerd.dll", "3033", now + chrono::Duration::seconds(30));
    assert!(
        event_is_recent(&jitter, now, BLOCK_RECENCY),
        "half a minute of clock jitter must not hide a real block"
    );

    let absurd = event("goworkerd.dll", "3033", now + chrono::Duration::days(1));
    assert!(
        !event_is_recent(&absurd, now, BLOCK_RECENCY),
        "a day in the future would suppress recovery forever"
    );
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

#[test]
fn the_parser_reads_the_timestamp_shapes_windows_actually_emits() {
    let mut bad = Vec::new();
    for shape in [
        "2026-09-22T18:30:45.1234567Z",
        "2026-09-22T18:30:45.123456700Z",
        "2026-09-22T18:30:45Z",
        "2026-09-22T18:30:45.123+00:00",
        "2026-09-22T18:30:45.1234567-05:00",
    ] {
        let xml = format!("<TimeCreated SystemTime='{shape}'/>");
        if event_time(&xml).is_none() {
            bad.push(shape);
        }
    }
    assert!(
        bad.is_empty(),
        "these real-world shapes do not parse: {bad:?}"
    );
}

/// T2's should-fix: `event_time` byte-sliced after reading a CHAR.
///
/// `recent_block` feeds this `String::from_utf8_lossy(&out.stdout)`, and lossy
/// conversion substitutes U+FFFD — three bytes — so an invalid byte immediately
/// after a literal `SystemTime=` made a one-byte slice land mid-character and
/// PANIC. That panic happens inside titan's health_check, which aborts the task,
/// so titan would stop being health-checked at all for the life of the process.
///
/// Reproduced by T2 verbatim: "byte index 1 is not a char boundary; it is
/// inside '\u{fffd}' (bytes 0..3)".
#[test]
fn a_lossy_byte_after_the_attribute_name_does_not_panic() {
    // Exactly what from_utf8_lossy produces for an invalid byte there.
    let lossy = format!(
        "<TimeCreated SystemTime={}2026-09-22T10:00:00Z'/>",
        '\u{fffd}'
    );

    // The assertion is that this RETURNS rather than unwinding.
    assert_eq!(event_time(&lossy), None);

    // And the whole scan over such a listing must survive it too, since that is
    // the path health_check actually takes.
    let now = chrono::Utc::now();
    let _ = first_block_for_at(&lossy, &titan_dll(), now, BLOCK_RECENCY);
}

/// A multi-byte character where the quote should be is the same class of bug,
/// reached without any lossy conversion at all. The property is that it does
/// not PANIC — extracting a well-formed value from between two multi-byte
/// delimiters is correct behaviour, not a failure, so this asserts on the
/// outcome rather than insisting on None.
#[test]
fn a_multibyte_character_where_the_quote_should_be_does_not_panic() {
    let at = "2026-09-22T10:00:00Z";
    for odd in ['\u{2019}', '\u{fffd}', 'é'] {
        let balanced = format!("<TimeCreated SystemTime={odd}{at}{odd}/>");
        // Delimited on both sides: the value is recoverable, and must be right.
        assert!(
            event_time(&balanced).is_some(),
            "a value delimited by {odd:?} should still be read"
        );

        // Unbalanced: nothing sane to extract, and still no panic.
        let unbalanced = format!("<TimeCreated SystemTime={odd}{at}'/>");
        assert_eq!(event_time(&unbalanced), None, "for {odd:?}");
    }
}
