//! Continuation #6, D-C6-1: a REGISTERED fryDVPN node that has no valid
//! FEM-FryNode firewall rule and whose wallet cannot pay the fee of its next
//! heartbeat shows the D-C5-2 setup line first, then, joined once with " · ",
//! the heartbeat shortfall — the same figures, from the same read, as D-C4-2.
//!
//! - frynode is still not spawned, and the node is never parked for funding.
//! - An unregistered node keeps the registration shortfall; an Unknown
//!   registration keeps the setup line alone (D-C5-6 variant C); a registered
//!   node WITH its rule keeps D-C4-2's notice alone.
//! - Every card-text matcher treats the combined line as the setup line.
//!
//! Expected lines are literals, never built by the code under test. Each
//! scenario runs in a child process; see `fryvpn_c4_support`.

use super::fryvpn_c4_support::*;
use super::*;
use crate::supervisor::health::{recovery_action, RecoveryAction};

const SETUP: &str = "Awaiting administrator action — Click Retry on the Security hardening \
                     banner to allow fryDVPN through Windows Firewall.";

/// The D-C6-1 line for a registered node short of `short` ALGO.
fn setup_and_heartbeat(short: &str) -> String {
    format!(
        "{SETUP} · This node's wallet also cannot pay the 0.001 ALGO fee of its next \
         heartbeat — send {short} ALGO to {ADDR}."
    )
}

#[test]
fn a_registered_node_short_of_its_next_heartbeat_waits_in_setup_with_the_heartbeat_text() {
    run_scenario(module_path!(), "registered_short");
}

#[test]
fn the_heartbeat_figure_is_this_checks_own_read_and_never_freezes() {
    run_scenario(module_path!(), "figure_follows_wallet");
}

#[test]
fn an_unregistered_short_node_keeps_the_registration_shortfall_only() {
    run_scenario(module_path!(), "unregistered_short");
}

#[test]
fn an_unknown_registration_keeps_the_setup_line_alone() {
    run_scenario(module_path!(), "registration_unknown");
}

#[test]
fn a_registered_node_with_its_rule_keeps_only_the_dc42_notice() {
    run_scenario(module_path!(), "registered_with_rule");
}

/// The lab example (amount 100_999, min 100_000) as the card shows it.
#[test]
fn every_rust_matcher_treats_the_combined_line_as_the_setup_line() {
    let combined = format!(
        "Awaiting administrator action — Click Retry on the Security hardening banner to \
         allow fryDVPN through Windows Firewall. · This node's wallet also cannot pay the \
         0.001 ALGO fee of its next heartbeat — send 0.000001 ALGO to {ADDR}."
    );
    for line in [SETUP, combined.as_str()] {
        assert!(crate::integrations::awaits_user_action(line), "{line}");
        assert!(!crate::integrations::upstream_unreachable(line), "{line}");
        assert_eq!(
            recovery_action(&HealthStatus::Unhealthy(line.to_string()), true, 0, 6),
            RecoveryAction::None,
            "{line}"
        );
    }
    let markers = |line: &str| {
        crate::integrations::AWAITING_MARKERS
            .iter()
            .filter(|m| line.contains(*m))
            .count()
    };
    assert_eq!(markers(&combined), markers(SETUP));
}

#[test]
#[ignore = "scenario entry point: the tests above run it in a child process"]
fn child() {
    let Ok(scenario) = std::env::var(SCENARIO_VAR) else {
        return;
    };
    match scenario.as_str() {
        "registered_short" => registered_short(),
        "figure_follows_wallet" => figure_follows_wallet(),
        "unregistered_short" => unregistered_short(),
        "registration_unknown" => registration_unknown(),
        "registered_with_rule" => registered_with_rule(),
        other => panic!("unknown scenario {other}"),
    }
    println!("{DONE} {scenario}");
}

fn set_rule(listing: Option<String>) {
    *FRYNODE_RULE_LISTING_FOR_TEST.lock().unwrap() = Some(listing);
}

fn missing_rule() -> Option<String> {
    Some("NET|Public\r\nEND\r\n".to_string())
}

fn valid_rule() -> Option<String> {
    let frynode = FryVpnIntegration::binary_path().expect("the scenario's frynode");
    Some(format!(
        "RULE|Inbound|True|Allow|Any|{frynode}\r\nNET|Public\r\nEND\r\n"
    ))
}

/// A start with the rule missing: it succeeds, spawns nothing, parks nothing.
fn start_without_rule(amount: u64, registry_box: RegistryBox) -> Scene {
    let s = Scene::new(World::new(amount, registry_box), Endpoint::PortInline, None);
    set_rule(missing_rule());
    let started = s.block_on(s.integ.start());
    assert!(
        started.is_ok() && !Scene::spawn_attempted(&started),
        "{amount}: frynode must not be spawned without its rule: {started:?}"
    );
    s
}

fn assert_setup_state(s: &Scene, expected: &str, context: &str) {
    let status = s.block_on(s.integ.health_check());
    assert_eq!(
        status,
        HealthStatus::Unhealthy(expected.to_string()),
        "{context}"
    );
    assert!(
        crate::integrations::awaits_user_action(expected),
        "{context}: the card must read Setup required"
    );
    assert_eq!(
        recovery_action(&status, true, 0, 6),
        RecoveryAction::None,
        "{context}: never a restart"
    );
    assert_eq!(s.parked(), None, "{context}: never a funding park");
}

/// The first health check, of `checks`, whose line is `expected`.
fn first_check_showing(
    s: &Scene,
    expected: &str,
    checks: std::ops::RangeInclusive<u32>,
) -> Option<u32> {
    checks.into_iter().find(|_| {
        s.block_on(s.integ.health_check()) == HealthStatus::Unhealthy(expected.to_string())
    })
}

fn registered_short() {
    // Lab figures: 999 µALGO spendable, one heartbeat costs 1000.
    let s = start_without_rule(100_999, RegistryBox::Present);
    let line = setup_and_heartbeat("0.000001");
    assert_eq!(line.matches(" · ").count(), 1, "joined once");
    assert!(!line.contains('\n'), "one line");
    for check in 1..=3 {
        assert_setup_state(&s, &line, &format!("check {check}"));
    }
    assert!(
        s.log_lines("registration deferred").is_empty(),
        "a registered node is never parked for funding"
    );
    assert!(
        !Scene::spawn_attempted(&s.block_on(s.integ.start())),
        "a second start still spawns nothing"
    );
    s.assert_nothing_submitted();
}

fn figure_follows_wallet() {
    let s = start_without_rule(100_014, RegistryBox::Present);
    assert_setup_state(&s, &setup_and_heartbeat("0.000986"), "check 1");

    // The figure moves with the wallet on the ordinary backoff (1, 2, …).
    s.set_world(|w| w.amount = 100_500);
    assert_eq!(
        first_check_showing(&s, &setup_and_heartbeat("0.000500"), 2..=6),
        Some(3),
        "re-read at the next due check"
    );

    // Exactly one heartbeat spendable: the heartbeat text leaves the line.
    s.set_world(|w| w.amount = 101_000);
    let at = first_check_showing(&s, SETUP, 4..=12).expect("the heartbeat text clears");

    // Short again, then a balance that cannot be read: the next due read
    // drops the figure rather than keep one it did not measure.
    s.set_world(|w| w.amount = 100_014);
    let seen = first_check_showing(&s, &setup_and_heartbeat("0.000986"), at + 1..=at + 12)
        .expect("short again");
    s.set_world(|w| w.account = Account::Html503);
    assert!(
        first_check_showing(&s, SETUP, seen + 1..=seen + 12).is_some(),
        "an unmeasured balance is never quoted"
    );
    assert_eq!(s.parked(), None);
    s.assert_nothing_submitted();
}

fn unregistered_short() {
    let s = start_without_rule(100_014, RegistryBox::Absent);
    let registration =
        registration_funding_message(100_014, MIN_BALANCE, REGISTRATION_MIN_MICROALGOS, ADDR)
            .expect_err("fixture: 100_014 cannot afford registration");
    let line = format!("{SETUP} · {registration}");
    assert!(!line.contains("also cannot pay"));
    for check in 1..=3 {
        assert_setup_state(&s, &line, &format!("check {check}"));
    }
    s.assert_nothing_submitted();
}

fn registration_unknown() {
    let s = start_without_rule(100_014, RegistryBox::Html503);
    for check in 1..=3 {
        assert_setup_state(&s, SETUP, &format!("check {check}"));
    }
    s.assert_nothing_submitted();
}

fn registered_with_rule() {
    let s = Scene::new(
        World::new(100_014, RegistryBox::Present),
        Endpoint::PortInline,
        None,
    );
    set_rule(valid_rule());
    let started = s.block_on(s.integ.start());
    assert!(Scene::spawn_attempted(&started), "{started:?}");
    assert_eq!(s.parked(), None);
    assert_eq!(*PARKED_RULE_REASON.lock().unwrap(), None);
    assert_eq!(
        funding_notice().as_deref(),
        Some(
            format!(
                "fryDVPN is running, but this node's wallet cannot pay the 0.001 ALGO fee of \
                 its next heartbeat — send 0.000986 ALGO to {ADDR}. The chain rejects each \
                 unpaid heartbeat and nothing is spent; heartbeats resume automatically once \
                 the wallet is funded."
            )
            .as_str()
        ),
        "D-C4-2 is unchanged for a node with its rule"
    );
    s.assert_nothing_submitted();
}
