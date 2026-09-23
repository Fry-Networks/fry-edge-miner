//! B3 defect 1/3/4: the gate's behaviour, proven without Windows, without a
//! UAC prompt and without any I/O.
//!
//! The gate's state is process-global by design, and `cargo test` runs these in
//! parallel, so every test uses its own `purpose` and its own `attempt_key`.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// The gate's UI-facing state for one purpose.
fn reason(purpose: &str) -> Option<String> {
    blocked_reasons().get(purpose).cloned()
}

/// B3 Done-when: "FEM-initiated elevation requests = 0 without user action".
/// The health loop's own shape: `max_restarts` attempts, then one more every
/// `REARM_TICKS` ticks, forever. Two hundred of those must raise nothing.
#[test]
fn an_automatic_trigger_never_elevates() {
    let ran = AtomicUsize::new(0);
    let mut skipped = 0usize;
    for tick in 0..200 {
        let key = format!("auto-never|tick-{tick}");
        let outcome = run_elevated(
            "test-automatic-never",
            &key,
            ElevationTrigger::Automatic,
            || {
                ran.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        );
        assert_eq!(outcome, Err(ElevationSkipped::NeedsApproval));
        skipped += 1;
    }
    assert_eq!(
        ran.load(Ordering::SeqCst),
        0,
        "no automatic tick may elevate"
    );
    assert_eq!(skipped, 200);
}

/// The decline memory: one prompt per request, not one per health tick.
#[test]
fn a_user_click_elevates_exactly_once_per_attempt_key() {
    let ran = AtomicUsize::new(0);
    let key = "once-per-key|C:\\FEM\\resources\\frynode.exe";

    let first = run_elevated(
        "test-once-per-key",
        key,
        ElevationTrigger::UserClick,
        || {
            ran.fetch_add(1, Ordering::SeqCst);
            Ok(7u32)
        },
    );
    assert_eq!(first, Ok(7));
    assert_eq!(ran.load(Ordering::SeqCst), 1);

    let second = run_elevated(
        "test-once-per-key",
        key,
        ElevationTrigger::UserClick,
        || {
            ran.fetch_add(1, Ordering::SeqCst);
            Ok(7u32)
        },
    );
    assert_eq!(second, Err(ElevationSkipped::AlreadyAttempted));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        1,
        "a second click on the same target must not raise a second prompt"
    );
}

/// The regression the "once per purpose" design would have caused: Olostep
/// self-updates into `app-X.Y.Z`, so the firewall rule's program path changes
/// and the new path legitimately needs its own one attempt.
#[test]
fn a_new_attempt_key_re_arms_a_user_click() {
    let ran = AtomicUsize::new(0);
    let old = "rearm|c:\\olostep\\app-1.2.3\\olostepbrowser.exe";
    let new = "rearm|c:\\olostep\\app-1.2.4\\olostepbrowser.exe";

    let _ = run_elevated("test-rearm", old, ElevationTrigger::UserClick, || {
        ran.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    let after_update = run_elevated("test-rearm", new, ElevationTrigger::UserClick, || {
        ran.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });

    assert_eq!(after_update, Ok(()));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        2,
        "a changed program path is a new request, not a repeat of the old one"
    );
}

/// B3 Done-when: "never concurrent". Serialised, and — the reason a plain
/// in-flight flag was rejected — never silently dropped.
#[test]
fn elevations_are_serialised_never_concurrent() {
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_seen = Arc::new(AtomicUsize::new(0));
    let ran = Arc::new(AtomicUsize::new(0));

    let handles: Vec<_> = (0..8)
        .map(|i| {
            let in_flight = Arc::clone(&in_flight);
            let max_seen = Arc::clone(&max_seen);
            let ran = Arc::clone(&ran);
            std::thread::spawn(move || {
                let key = format!("serialised|{i}");
                let _ = run_elevated("test-serialised", &key, ElevationTrigger::UserClick, || {
                    let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                    ran.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                });
            })
        })
        .collect();
    for h in handles {
        h.join().expect("worker thread panicked");
    }

    assert_eq!(
        max_seen.load(Ordering::SeqCst),
        1,
        "two UAC prompts must never be in flight at once"
    );
    assert_eq!(
        ran.load(Ordering::SeqCst),
        8,
        "serialised means waited, not dropped"
    );
}

/// Defect 5's failure mode, classified: `output_bounded` kills the requesting
/// PowerShell at its deadline and returns `TimedOut`. Nobody approved, so the
/// gate must record that as a decline rather than as a broken install.
#[test]
fn a_timed_out_prompt_is_recorded_as_declined() {
    assert!(is_declined(None, Some(std::io::ErrorKind::TimedOut)));
    // The outer wrapper's own codes (security_setup::hardening_outcome) and
    // Windows' ERROR_CANCELLED.
    assert!(is_declined(Some(2), None));
    assert!(is_declined(Some(3), None));
    assert!(is_declined(Some(1223), None));
    // A real failure of the elevated work is NOT a decline.
    assert!(!is_declined(Some(1603), None));
    assert!(!is_declined(None, Some(std::io::ErrorKind::NotFound)));
    assert!(!is_declined(None, None));
}

/// Defect 4: the Done-when's required state has to actually reach the UI.
#[test]
fn a_suppressed_or_declined_elevation_publishes_the_needs_approval_string() {
    let suppressed = run_elevated(
        "test-publishes-suppressed",
        "publish|suppressed",
        ElevationTrigger::Automatic,
        || Ok(()),
    );
    assert_eq!(suppressed, Err(ElevationSkipped::NeedsApproval));
    assert_eq!(
        reason("test-publishes-suppressed").as_deref(),
        Some(NEEDS_APPROVAL_MESSAGE)
    );

    let declined: Result<(), ElevationSkipped> = run_elevated(
        "test-publishes-declined",
        "publish|declined",
        ElevationTrigger::UserClick,
        || {
            Err(anyhow::Error::new(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "the consent prompt was never answered",
            )))
        },
    );
    assert_eq!(
        declined,
        Err(ElevationSkipped::Failed(NEEDS_APPROVAL_MESSAGE.to_string()))
    );
    assert_eq!(
        reason("test-publishes-declined").as_deref(),
        Some(NEEDS_APPROVAL_MESSAGE)
    );
}

/// A genuine failure of the elevated work must keep its own message — telling
/// a user to approve something they already approved is a dead end.
#[test]
fn a_genuine_failure_keeps_its_own_message() {
    let failed: Result<(), ElevationSkipped> = run_elevated(
        "test-genuine-failure",
        "genuine|failure",
        ElevationTrigger::UserClick,
        || anyhow::bail!("netsh exited 1603"),
    );
    assert_eq!(
        failed,
        Err(ElevationSkipped::Failed("netsh exited 1603".to_string()))
    );
    assert_eq!(
        reason("test-genuine-failure").as_deref(),
        Some("netsh exited 1603")
    );
}

/// A successful elevation clears the card, and so does the retry gesture.
#[test]
fn a_success_and_an_explicit_clear_both_drop_the_block() {
    let _ = run_elevated(
        "test-clears",
        "clears|first",
        ElevationTrigger::Automatic,
        || Ok(()),
    );
    assert!(reason("test-clears").is_some());

    let ok = run_elevated(
        "test-clears",
        "clears|second",
        ElevationTrigger::UserClick,
        || Ok(()),
    );
    assert_eq!(ok, Ok(()));
    assert_eq!(reason("test-clears"), None);

    let _ = run_elevated(
        "test-clears",
        "clears|third",
        ElevationTrigger::Automatic,
        || Ok(()),
    );
    assert!(reason("test-clears").is_some());
    clear_blocked("test-clears");
    assert_eq!(reason("test-clears"), None);
}

// B3 defect 3 (`ensure_docker`'s "never at app boot" contract) used to have a
// test here called the_docker_installer_never_runs_on_an_automatic_trigger.
// DELETED as vacuous: it passed "docker-desktop" and Automatic to the gate and
// asserted the closure did not run — but the gate treats those strings like any
// others, so it was a duplicate of an_automatic_trigger_never_elevates wearing
// Docker-flavoured labels. It could not fail for any Docker-specific reason; if
// ensure_docker stopped passing Automatic it would still have passed, while its
// name told a reader the contract was pinned here.
//
// The real property is which trigger the CALL SITES pass, which is not visible
// from inside this module. integrations::docker_boot_contract_tests reads them
// directly (the_default_ensure_docker_uses_an_automatic_trigger,
// the_docker_installer_takes_the_callers_trigger,
// no_start_impl_asks_docker_for_a_user_click), so the property is pinned where
// it can actually be observed.

/// THE RETRY GESTURE, end to end. Before this, `clear_blocked` wiped only the
/// card message while the spent attempt key stayed in the gate, so the user's
/// toggle produced no prompt, no rule, and an empty card — the B14
/// silent-revert shape, caused by the B3 fix. D-02 ships no Retry button
/// precisely because the toggle IS the gesture, so this path has to work.
#[test]
fn the_retry_gesture_re_arms_the_one_allowed_attempt() {
    let ran = AtomicUsize::new(0);
    let key = "retry-gesture|C:\\FEM\\resources\\frynode.exe";
    let declined = || -> anyhow::Result<()> {
        ran.fetch_add(1, Ordering::SeqCst);
        Err(anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "the user cancelled the consent prompt",
        )))
    };

    // The user clicks, and declines the prompt.
    let first = run_elevated(
        "test-retry-gesture",
        key,
        ElevationTrigger::UserClick,
        declined,
    );
    assert_eq!(
        first,
        Err(ElevationSkipped::Failed(NEEDS_APPROVAL_MESSAGE.to_string()))
    );
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    assert_eq!(
        reason("test-retry-gesture").as_deref(),
        Some(NEEDS_APPROVAL_MESSAGE)
    );

    // Clicking again WITHOUT a gesture must not re-raise it — that is the
    // anti-loop guarantee, and the card must not fall silent either.
    let again = run_elevated(
        "test-retry-gesture",
        key,
        ElevationTrigger::UserClick,
        declined,
    );
    assert_eq!(again, Err(ElevationSkipped::AlreadyAttempted));
    assert_eq!(ran.load(Ordering::SeqCst), 1, "no prompt without a gesture");
    assert_eq!(
        reason("test-retry-gesture").as_deref(),
        Some(NEEDS_APPROVAL_MESSAGE),
        "the card must keep saying what is wrong, not go blank"
    );

    // Now the gesture: toggle off/on calls clear_blocked.
    clear_blocked("test-retry-gesture");
    assert_eq!(reason("test-retry-gesture"), None);

    let retried = run_elevated(
        "test-retry-gesture",
        key,
        ElevationTrigger::UserClick,
        || {
            ran.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
    );
    assert_eq!(retried, Ok(()), "the gesture must re-arm the one attempt");
    assert_eq!(
        ran.load(Ordering::SeqCst),
        2,
        "the retry must actually raise the prompt again"
    );
}

/// …and the gesture re-arms ONLY the purpose it names.
#[test]
fn the_retry_gesture_does_not_re_arm_another_integrations_attempt() {
    let ran = AtomicUsize::new(0);
    let spend = |purpose: &'static str| -> Result<(), ElevationSkipped> {
        run_elevated(
            purpose,
            "shared-key-shape",
            ElevationTrigger::UserClick,
            || {
                ran.fetch_add(1, Ordering::SeqCst);
                anyhow::bail!("declined")
            },
        )
    };
    let _ = spend("test-retry-scope-a");
    let _ = spend("test-retry-scope-b");
    assert_eq!(ran.load(Ordering::SeqCst), 2);

    clear_blocked("test-retry-scope-a");

    assert!(
        matches!(
            spend("test-retry-scope-a"),
            Err(ElevationSkipped::Failed(_))
        ),
        "the cleared purpose must re-arm"
    );
    assert_eq!(
        spend("test-retry-scope-b"),
        Err(ElevationSkipped::AlreadyAttempted),
        "clearing one card must not re-arm another's elevation"
    );
    assert_eq!(ran.load(Ordering::SeqCst), 3);
}
