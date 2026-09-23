//! B16 — Titan exits / upstream network.
//!
//! The reported line is an UPSTREAM condition:
//! `2026-09-18T19:56:30.482-0500 ERROR main titan-edge/main.go:102 RPC client
//! error: sendRequest failed: Post ".../rpc/v0": timeout: no recent network
//! activity`. FEM answered it by killing a titan-edge that was alive and
//! working, and then kept answering it long after the condition had cleared,
//! because the log tail is a LINE window with no notion of time.

use super::*;

const REPORTED_LINE: &str = "ERROR main titan-edge/main.go:102 RPC client error: sendRequest failed: Post \"https://cassini-locator.titannet.io:5000/rpc/v0\": timeout: no recent network activity";

fn stamped(at: chrono::DateTime<chrono::Utc>) -> String {
    // titan-edge's own format: an offset with NO colon, so it is not rfc3339.
    format!("{} {REPORTED_LINE}", at.format("%Y-%m-%dT%H:%M:%S%.3f%z"))
}

/// `now`, truncated to a whole second.
///
/// The log format carries milliseconds, so a raw `Utc::now()` loses its
/// sub-millisecond digits when written into a fixture — which makes a stamp
/// built as `now - window` land a fraction of a millisecond EARLIER than
/// intended and turns the boundary case into a coin flip. Truncating first
/// makes the round-trip exact, so the edge test measures the boundary rather
/// than formatting precision.
fn now_truncated() -> chrono::DateTime<chrono::Utc> {
    use chrono::Timelike;
    chrono::Utc::now()
        .with_nanosecond(0)
        .expect("second is valid")
}

fn window() -> chrono::Duration {
    chrono::Duration::minutes(ERROR_RECENCY_WINDOW_MINUTES)
}

#[test]
fn a_stale_error_outside_the_window_no_longer_counts_as_a_failing_tick() {
    let now = chrono::Utc::now();
    let log = stamped(now - chrono::Duration::minutes(30));

    // The pre-fix finder still matches it — that is the defect, stated as an
    // assertion rather than as prose.
    assert!(
        first_error_line(&log).is_some(),
        "characterization: the unfiltered finder matches a 30-minute-old line forever"
    );
    assert!(
        first_recent_error_line(&log, now, window()).is_none(),
        "a condition from 30 minutes ago must not keep failing every tick"
    );
}

#[test]
fn a_recent_error_still_counts() {
    let now = chrono::Utc::now();
    let log = stamped(now - chrono::Duration::seconds(10));

    assert!(first_recent_error_line(&log, now, window()).is_some());
}

/// Exactly on the boundary must still count: the window is inclusive, so a
/// steadily-failing daemon cannot slip through by landing on the edge.
#[test]
fn an_error_exactly_at_the_window_edge_still_counts() {
    let now = now_truncated();
    let log = stamped(now - window());

    assert!(first_recent_error_line(&log, now, window()).is_some());
}

/// Fail OPEN: every line that is not titan-shaped keeps behaving exactly as it
/// does today, so no existing diagnosis is lost.
#[test]
fn an_untimestamped_error_line_fails_open() {
    let now = chrono::Utc::now();
    let log = "ERROR rpc timeout: dial tcp 47.76.123.118:3456: i/o timeout";

    assert!(first_recent_error_line(log, now, window()).is_some());
    assert!(error_line_is_recent(log, now, window()));
    assert!(error_line_is_recent("", now, window()));
    assert!(error_line_is_recent(
        "not-a-timestamp ERROR boom",
        now,
        window()
    ));
}

/// A benign line must not become a failure just because it is recent.
#[test]
fn a_recent_but_benign_line_is_not_an_error() {
    let now = chrono::Utc::now();
    let log = format!(
        "{} INFO main scheduler connected errors=0",
        now.format("%Y-%m-%dT%H:%M:%S%.3f%z")
    );

    assert!(first_recent_error_line(&log, now, window()).is_none());
}

/// The reason string the card shows must still carry the real error, and must
/// still be recognised as an upstream condition rather than a device fault.
#[test]
fn the_upstream_reason_is_exempt_from_restarting_a_live_daemon() {
    let reason = daemon_log_failure_reason(&stamped(chrono::Utc::now()));

    assert!(
        crate::integrations::upstream_unreachable(&reason),
        "must be recognised as upstream: {reason}"
    );
    assert_eq!(
        crate::supervisor::health::recovery_action(&HealthStatus::Unhealthy(reason), true, 0, 6),
        crate::supervisor::health::RecoveryAction::None,
        "FEM must not TerminateProcess a live titan-edge over a network condition"
    );
}

/// The exemption must be narrow: a real local failure still gets restarted.
#[test]
fn an_ordinary_daemon_failure_is_still_restarted() {
    assert!(!crate::integrations::upstream_unreachable(
        "storagenode exited with code 1"
    ));
    assert_eq!(
        crate::supervisor::health::recovery_action(
            &HealthStatus::Unhealthy("storagenode exited with code 1".to_string()),
            true,
            0,
            6
        ),
        crate::supervisor::health::RecoveryAction::Restart
    );
}

/// G4 finding 8 — the gate and the reason used DIFFERENT predicates to pick a
/// line, so they could describe different failures.
///
/// Reproduced against the real functions before the fix: a fresh local
/// `{"level":"fatal","msg":"cannot open datastore"}` fired the recency-aware
/// gate, while `daemon_log_failure_reason` re-scanned with `first_error_line`
/// (no recency) and returned an hour-old connectivity line. That reason
/// matched `upstream_unreachable`, so `recovery_action` returned None and a
/// genuine device-side fault was never restarted — painted as a network
/// problem instead.
#[test]
fn a_local_fatal_is_not_reported_as_an_upstream_failure() {
    let now = now_truncated();
    let stale = (now - chrono::Duration::minutes(60))
        .format("%Y-%m-%dT%H:%M:%S%.3f%z")
        .to_string();
    // Stale connectivity line FIRST, which is what first_error_line returns.
    let combined = format!(
        "{stale} {REPORTED_LINE}\n{}\n",
        r#"{"level":"fatal","msg":"cannot open datastore"}"#
    );

    let selected = first_recent_error_line(&combined, now, window())
        .expect("the fresh local fatal must fire the gate");
    assert!(
        selected.contains("cannot open datastore"),
        "the gate selected {selected:?}"
    );

    let reason = daemon_log_failure_reason_for_line(selected);

    assert!(
        reason.contains("cannot open datastore"),
        "the card must describe the failure that actually fired the gate: {reason}"
    );
    assert!(
        !crate::integrations::upstream_unreachable(&reason),
        "a local fatal must not be exempted from recovery as an upstream condition: {reason}"
    );
    assert_eq!(
        crate::supervisor::health::recovery_action(&HealthStatus::Unhealthy(reason), true, 0, 6),
        crate::supervisor::health::RecoveryAction::Restart,
        "a genuine device-side fault must still be restarted"
    );
}

/// And the exemption must still work when the recent failure really IS
/// upstream — the fix must not have narrowed D-14 into uselessness.
#[test]
fn a_recent_upstream_failure_is_still_exempt() {
    let now = now_truncated();
    let combined = stamped(now);

    let selected = first_recent_error_line(&combined, now, window()).unwrap();
    let reason = daemon_log_failure_reason_for_line(selected);

    assert!(
        crate::integrations::upstream_unreachable(&reason),
        "{reason}"
    );
    assert_eq!(
        crate::supervisor::health::recovery_action(&HealthStatus::Unhealthy(reason), true, 0, 6),
        crate::supervisor::health::RecoveryAction::None
    );
}

/// The whole-log entry point keeps its behaviour, so bug3_daemon_error_tests
/// stay byte-identical and green.
#[test]
fn the_whole_log_entry_point_still_delegates_to_the_same_formatting() {
    let line = "2026-09-18T19:56:30.482-0500 ERROR main x.go:1 dial tcp: i/o timeout";
    assert_eq!(
        daemon_log_failure_reason(line),
        daemon_log_failure_reason_for_line(line)
    );
    assert_eq!(
        daemon_log_failure_reason(""),
        "Titan Network: the daemon reported a problem"
    );
}
