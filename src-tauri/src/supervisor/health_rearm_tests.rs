//! B10 defect 3: the restart-pause resume was bounded in code but nowhere else.
//!
//! `REARM_TICKS` was a `const` in the body of `health_check_loop`, so no test
//! could reach it and no caller could set it, and the card said only
//! "retrying periodically" — which names no period. A user watching a red card
//! had no way to tell a bounded pause from a dead one, and the Done-when asks
//! for a resume that is "bounded, documented, tested". Only the first of the
//! three was true.
//!
//! Separate file so health.rs's own three test modules stay byte-identical.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::health::{health_check_loop, HealthCheckConfig};
use crate::integrations::HealthStatus;

/// Short everything. At the shipped defaults the loop sleeps 5 + 15 + 45s
/// before the pause is even reached, so a test on real timings could not run.
fn fast_config(rearm_ticks: u32) -> HealthCheckConfig {
    HealthCheckConfig {
        check_interval: Duration::from_millis(20),
        max_restarts: 3,
        backoff_base: Duration::from_millis(1),
        starting_timeout_ticks: 0,
        rearm_ticks,
    }
}

/// The RED→GREEN one. Pre-fix the reason ends "retrying periodically"; the
/// user is told a period exists but never what it is.
#[tokio::test]
async fn the_paused_card_states_when_the_next_attempt_happens() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let restarts = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&restarts);

    tokio::spawn(health_check_loop(
        "fryvpn".to_string(),
        fast_config(10),
        || HealthStatus::Unhealthy("frynode process is not running".to_string()),
        move || {
            counter.fetch_add(1, Ordering::SeqCst);
            false
        },
        || true,
        tx,
    ));

    let paused = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = rx.recv().await {
            if let HealthStatus::Unhealthy(reason) = &event.status {
                if reason.contains("automatic restarts paused") {
                    return reason.clone();
                }
            }
        }
        panic!("the loop closed without ever pausing");
    })
    .await
    .expect("the pause event must arrive — an unbounded pause is the defect");

    assert!(
        paused.contains("the next automatic attempt is in"),
        "a paused card must say WHEN it resumes, not just that it will: {paused}"
    );
    assert!(
        paused.contains("automatic restarts paused"),
        "the substring downstream code greps for must survive: {paused}"
    );
    assert!(
        paused.contains("frynode process is not running"),
        "the original reason must not be swallowed by the pause text: {paused}"
    );
}

/// The interval is now settable, and its default is the shipped behaviour.
#[test]
fn the_rearm_interval_is_configurable_and_defaults_to_ten_ticks() {
    assert_eq!(HealthCheckConfig::default().rearm_ticks, 10);
}

/// The figure on the card is DERIVED, so it stays true if any input is
/// retuned. At the shipped defaults it is the documented 345s:
/// 10 x 30s of pause, one more 30s check, then 5s * 3^2 of backoff.
#[test]
fn the_advertised_interval_is_derived_from_the_real_timings() {
    let reason = super::health::pause_reason("x", &HealthCheckConfig::default());
    assert!(
        reason.contains("345s"),
        "defaults must advertise the real 345s resume: {reason}"
    );

    let retuned = HealthCheckConfig {
        check_interval: Duration::from_secs(10),
        rearm_ticks: 3,
        backoff_base: Duration::from_secs(1),
        max_restarts: 2,
        starting_timeout_ticks: 6,
    };
    // 3 * 10s + 1s * 3^1 = 33s
    assert!(
        super::health::pause_reason("x", &retuned).contains("33s"),
        "the figure must be computed, never hard-coded: {}",
        super::health::pause_reason("x", &retuned)
    );
}

/// COVERAGE, not a RED→GREEN proof: this passes before and after the change.
/// It is what makes the Done-when's "tested" true — a regression to an
/// unbounded pause FAILS here rather than hanging, because of the timeout.
#[tokio::test]
async fn restarts_resume_after_a_bounded_pause() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let restarts = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&restarts);

    tokio::spawn(health_check_loop(
        "fryvpn".to_string(),
        fast_config(3),
        || HealthStatus::Unhealthy("frynode process is not running".to_string()),
        move || {
            counter.fetch_add(1, Ordering::SeqCst);
            false
        },
        || true,
        tx,
    ));

    // Drain events so the channel cannot fill and stall the loop.
    tokio::spawn(async move { while rx.recv().await.is_some() {} });

    let resumed = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if restarts.load(Ordering::SeqCst) > 3 {
                return restarts.load(Ordering::SeqCst);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;

    let count = resumed.expect(
        "automatic restarts must RESUME after the pause — a pause that never \
         re-arms is exactly what users reported as a card stuck red forever",
    );
    assert!(
        count > 3,
        "the budget is 3; a resumed loop must exceed it, got {count}"
    );
}
