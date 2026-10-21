//! G2 (rc39b): Diiisco's enable intent reaches the config BEFORE the
//! install/start step begins, and a generic (non-rejection) failure puts the
//! previous value back. Rejections and timeouts behave as in A5.
use super::{
    apply_enable_intent, apply_intent_restore, should_persist_timed_out_enable,
    should_restore_intent_after_failed_enable, TOGGLE_STEP_TIMEOUT,
};
use crate::integrations::diiisco::NodeRejected;
use std::collections::HashMap;
use std::time::Duration;

fn rejected() -> anyhow::Error {
    anyhow::Error::new(NodeRejected(
        "the new Diiisco node did not prove the device key".into(),
    ))
}

fn toggle_src() -> &'static str {
    include_str!("integration.rs")
        .split("pub async fn toggle_integration")
        .nth(1)
        .unwrap()
        .split("pub async fn force_reinstall_integration")
        .next()
        .unwrap()
}

/// (a) the intent write comes before both slow awaits.
#[test]
fn the_intent_write_precedes_the_install_and_start_awaits() {
    let t = toggle_src();
    let write = t
        .find("persist_early_diiisco_intent(")
        .expect("the early intent write");
    let install = t.find("timeout(INSTALL_STEP_TIMEOUT").unwrap();
    let start = t.find("timeout(TOGGLE_STEP_TIMEOUT").unwrap();
    assert!(write < install && write < start);
}

/// (a) and it runs on the enable path only, after the pre-checks.
#[test]
fn the_early_write_sits_inside_the_enable_branch_after_the_requirements_gate() {
    let t = toggle_src();
    let gate = t.find("integration.check_requirements()").unwrap();
    let write = t.find("persist_early_diiisco_intent(").unwrap();
    let off = t.find("stop_for_disable()").unwrap();
    assert!(gate < write && write < off);
}

/// (b) the restore decision: generic errors restore, a rejection keeps.
#[test]
fn a_generic_error_restores_the_previous_intent_and_a_rejection_does_not() {
    for msg in [
        "Diiisco node image rebuild is backing off after a failed attempt",
        "docker is not installed",
        "docker compose build failed: boom",
    ] {
        assert!(
            should_restore_intent_after_failed_enable("diiisco", &anyhow::anyhow!(msg)),
            "{msg}"
        );
    }
    assert!(!should_restore_intent_after_failed_enable(
        "diiisco",
        &rejected()
    ));
    assert!(!should_restore_intent_after_failed_enable(
        "diiisco",
        &rejected().context("start failed")
    ));
    assert!(!should_restore_intent_after_failed_enable(
        "sentinel",
        &anyhow::anyhow!("x")
    ));
}

/// (b) restore by key: absent -> removed, present -> its old value.
#[test]
fn the_restore_is_by_key() {
    let mut m = HashMap::new();
    let prior = apply_enable_intent(&mut m, "diiisco");
    assert_eq!(prior, None);
    assert_eq!(m.get("diiisco"), Some(&true));
    apply_intent_restore(&mut m, "diiisco", prior);
    assert!(!m.contains_key("diiisco"));

    let mut m = HashMap::from([("diiisco".to_string(), false), ("aem".to_string(), true)]);
    let prior = apply_enable_intent(&mut m, "diiisco");
    assert_eq!(prior, Some(false));
    assert!(m["diiisco"]);
    apply_intent_restore(&mut m, "diiisco", prior);
    assert!(!m["diiisco"]);
    assert!(m["aem"], "other keys untouched");
}

/// F2: compare-and-set. A toggle-off (or any other write) that landed while
/// the enable was in flight is never overwritten by the restore.
#[test]
fn the_restore_only_acts_while_the_intent_is_still_the_one_this_call_wrote() {
    for prior in [Some(true), None] {
        let mut m = HashMap::from([("diiisco".to_string(), false)]);
        apply_intent_restore(&mut m, "diiisco", prior);
        assert_eq!(m.get("diiisco"), Some(&false), "prior {prior:?}");
    }
    // The key was removed meanwhile: nothing to restore either.
    let mut m: HashMap<String, bool> = HashMap::new();
    apply_intent_restore(&mut m, "diiisco", Some(true));
    assert!(m.is_empty());
}

/// Both failure arms call the restore, after the rejection helper and before
/// returning.
#[test]
fn both_error_arms_restore_after_the_rejection_helper_and_before_returning() {
    let t = toggle_src();
    for call in [
        "timeout(INSTALL_STEP_TIMEOUT",
        "timeout(TOGGLE_STEP_TIMEOUT",
    ] {
        let arm = &t[t.find(call).unwrap()..];
        let rej = arm.find("persist_rejected_diiisco_enable(").unwrap();
        let res = arm.find("restore_intent_after_failed_enable(").unwrap();
        let ret = arm.find("return Err(err_msg)").unwrap();
        assert!(rej < res && res < ret, "{call}");
    }
}

/// CHARACTERIZATION (passes on the old code): a rejection that arrives after
/// the toggle bound surfaces as a TIMEOUT, and the timeout arm persists the
/// intent for Diiisco. Same structure as the production await, shorter bound.
#[tokio::test]
async fn characterization_a_rejection_after_the_bound_is_persisted_via_the_timeout_arm() {
    assert_eq!(TOGGLE_STEP_TIMEOUT, Duration::from_secs(60));
    let late_rejection = async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        Err::<(), anyhow::Error>(rejected())
    };
    match tokio::time::timeout(Duration::from_millis(50), late_rejection).await {
        Ok(_) => panic!("the bound must elapse first"),
        Err(_) => assert!(should_persist_timed_out_enable("diiisco")),
    }
    let t = toggle_src();
    let start = &t[t.find("timeout(TOGGLE_STEP_TIMEOUT").unwrap()..];
    let arm = &start[start.find("Err(_) =>").unwrap()..];
    assert!(
        arm.find("persist_timed_out_diiisco_enable(").unwrap()
            < arm.find("return Err(err_msg)").unwrap()
    );
}
