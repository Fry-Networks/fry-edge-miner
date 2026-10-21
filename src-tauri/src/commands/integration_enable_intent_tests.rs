//! A5: a Diiisco enable that was REJECTED (the fresh node did not prove the
//! device key) leaves the user's intent in the config, so the next launch tries
//! again. Only that outcome, only Diiisco: a timeout, a stop or any other
//! integration's failure persists nothing.
use super::{should_persist_rejected_enable, should_persist_timed_out_enable};
use crate::integrations::diiisco::NodeRejected;

fn rejected() -> anyhow::Error {
    anyhow::Error::new(NodeRejected(
        "the new Diiisco node did not prove the device key on start; nothing was promoted".into(),
    ))
}

#[test]
fn a_rejected_diiisco_enable_is_persisted() {
    assert!(should_persist_rejected_enable("diiisco", &rejected()));
    // Still a rejection when context was added on the way up.
    assert!(should_persist_rejected_enable(
        "diiisco",
        &rejected().context("start failed")
    ));
}

#[test]
fn nothing_else_is_persisted() {
    // another integration, even with the same error type
    assert!(!should_persist_rejected_enable("sentinel", &rejected()));
    // a timeout, a stop, a plain failure
    for msg in [
        "Diiisco install timed out",
        "Diiisco install was stopped; nothing was promoted",
        "docker compose build failed: boom",
    ] {
        assert!(
            !should_persist_rejected_enable("diiisco", &anyhow::anyhow!(msg)),
            "{msg}"
        );
    }
}

/// Both Err arms of the enable path (install, then start) reach the helper
/// before returning the error.
#[test]
fn both_enable_error_arms_persist_a_rejection_before_returning() {
    let src = include_str!("integration.rs");
    let toggle = src
        .split("pub async fn toggle_integration")
        .nth(1)
        .unwrap()
        .split("pub async fn force_reinstall_integration")
        .next()
        .unwrap();
    for call in ["install_for_user()", "start_for_user()"] {
        let at = toggle.find(call).unwrap_or_else(|| panic!("no {call}"));
        let arm = &toggle[at..];
        let persist = arm
            .find("persist_rejected_diiisco_enable(")
            .unwrap_or(usize::MAX);
        let ret = arm.find("return Err(err_msg)").unwrap();
        assert!(persist < ret, "{call}: persist must come before the return");
    }
}

/// In the real UI flow the rejection often arrives after the 60 s toggle bound,
/// so the enable shows up as a TIMEOUT. For Diiisco only, that keeps the
/// intent too; every other integration keeps today's behavior.
#[test]
fn a_diiisco_enable_that_times_out_keeps_the_intent_and_nothing_else_does() {
    assert!(should_persist_timed_out_enable("diiisco"));
    for id in [
        "sentinel",
        "mysterium",
        "pawns",
        "aem",
        "fryvpn",
        "Diiisco",
        "",
    ] {
        assert!(!should_persist_timed_out_enable(id), "{id}");
    }
}

/// Both timeout arms (install, then start) persist before returning.
#[test]
fn both_timeout_arms_persist_a_diiisco_intent_before_returning() {
    let src = include_str!("integration.rs");
    let toggle = src
        .split("pub async fn toggle_integration")
        .nth(1)
        .unwrap()
        .split("pub async fn force_reinstall_integration")
        .next()
        .unwrap();
    for call in ["install_for_user()", "start_for_user()"] {
        let at = toggle.find(call).unwrap_or_else(|| panic!("no {call}"));
        let after = &toggle[at..];
        let arm = after.find("Err(_) =>").unwrap();
        let after = &after[arm..];
        let persist = after
            .find("persist_timed_out_diiisco_enable(")
            .unwrap_or(usize::MAX);
        let ret = after.find("return Err(err_msg)").unwrap();
        assert!(persist < ret, "{call}: persist must come before the return");
    }
}
