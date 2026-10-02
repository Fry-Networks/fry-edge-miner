//! BL-1 — every ordinary FEM quit recorded a Pawns §5.8 withdrawal.
//!
//! B18 fixed the SUPERVISOR path: recycling a container no longer appends a
//! `withdrawal` to the durable consent log. D-03, landing in the same release,
//! added a courtesy exit path so partners get a graceful stop instead of being
//! hard-killed by the job object. Each is correct alone. Together, the exit path
//! called the bare `stop()`, which for Pawns means `StopReason::UserDisable`, so
//! quitting FEM revoked a consent nobody withdrew — B18's symptom with the period
//! changed from ~1-2 h to once per launch.
//!
//! The pre-existing suite could not catch it. `pawns_restart_consent_tests.rs`
//! re-implements `stop_inner`'s branch in the test body:
//!
//! ```ignore
//! if stop_records_withdrawal(StopReason::SupervisorRestart) {
//!     PawnsIntegration::record_consent_event_at(&path, "withdrawal");
//! }
//! ```
//!
//! The predicate is false for a restart, so the guarded block never runs and
//! "the log is byte-identical" is trivially true — it would pass with
//! `stop_inner` completely broken, because it never calls it. And nothing
//! asserted WHICH stop method the exit path chooses, which is the single fact
//! that decides this bug.
//!
//! Needles are assembled at runtime so these guards cannot be satisfied by their
//! own text, matching the idiom in `pawns_restart_consent_tests.rs`.

use super::*;

/// The consent effect of the reason the EXIT path resolves to.
/// This is the unit the bug lived in: `UserDisable` writes a withdrawal,
/// and the exit path must not resolve to it.
#[test]
fn the_exit_reason_must_not_be_the_one_that_records_a_withdrawal() {
    assert!(
        stop_records_withdrawal(StopReason::UserDisable),
        "guard integrity: UserDisable must still record a withdrawal, or this test proves nothing"
    );
    assert!(
        !stop_records_withdrawal(StopReason::SupervisorRestart),
        "the non-owner reason must not record a withdrawal"
    );
}

/// THE ASSERTION WHOSE ABSENCE ALLOWED BL-1: which method does the exit path call?
/// Reads the real `main.rs`. Fails on the pre-fix source, where the call was
/// `integration.stop()`.
#[test]
fn the_exit_path_does_not_call_the_bare_stop() {
    let src = include_str!("../main.rs");
    let at = src
        .find(&format!("fn stop_partners{}(", "_gracefully"))
        .expect("stop_partners_gracefully must exist");
    // Bound the window at the end of the spawned stop loop.
    let end = src[at..]
        .find("let _ = tx.send(());")
        .map(|e| at + e)
        .unwrap_or(src.len());
    let body = &src[at..end];

    let bare = format!("integration.stop{}", "()");
    let scoped = format!("integration.stop_for{}()", "_exit");

    assert!(
        body.contains(&scoped),
        "the exit path must stop partners with the exit-scoped method:\n{body}"
    );
    assert!(
        !body.contains(&bare),
        "the exit path still calls the bare stop(), which for Pawns is \
         StopReason::UserDisable and appends a §5.8 withdrawal on every quit:\n{body}"
    );
}

/// And the exit-scoped method must not be defined as a synonym for the
/// user-disable path, or the fix would be inert. fryDVPN overrides
/// `stop_for_disable` to deregister ON CHAIN; routing exit through it would make
/// every quit an on-chain transaction.
#[test]
fn the_exit_scoped_stop_defaults_away_from_disable() {
    let src = include_str!("mod.rs");
    let at = src
        .find(&format!("async fn stop_for{}(&self)", "_exit"))
        .expect("the trait must declare an exit-scoped stop");
    let end = src[at..]
        .find("\n    /// Stop this integration because the USER")
        .map(|e| at + e)
        .unwrap_or(src.len());
    let body = &src[at..end];

    let disable = format!("stop_for{}()", "_disable");
    assert!(
        !body.contains(&disable),
        "exit must not route through the disable path — fryDVPN deregisters on chain there:\n{body}"
    );
    let restart = format!("stop_for{}()", "_restart");
    assert!(
        body.contains(&restart),
        "exit should default to the restart-scoped stop, which declines to record a withdrawal:\n{body}"
    );
}
