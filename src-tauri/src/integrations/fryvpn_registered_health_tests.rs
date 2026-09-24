//! D-C4-2 rework — a registered node's health is frynode's own, whatever its
//! balance, so the PoC document counts it exactly as 0.4.33 did.
//!
//! Reporting a heartbeat shortfall as an Unhealthy status fed `build_poc_doc`:
//! fryDVPN dropped out of the healthy count and the proportion, and on a device
//! where it was the only healthy integration the slot multiplier fell from 1.0
//! to 0.0 — a client-side reward change. The shortfall is a UI-only notice
//! (see `fryvpn_funding_notice_tests`); these tests pin that health, and the
//! PoC document built from it, never see the balance.
//!
//! Both need a frynode stand-in that stays alive under frynode's own flags,
//! which only a script provides, so they run on unix. Each runs in a child
//! process; see `fryvpn_c4_support`.

#[cfg(unix)]
use super::fryvpn_c4_support::*;
#[cfg(unix)]
use super::*;

/// The PoC document for a device whose only integration is this fryDVPN.
#[cfg(unix)]
fn assert_counted_healthy(s: &Scene, when: &str) {
    let doc = s.poc_doc();
    assert!(
        doc.integrations.get("fryvpn").is_some_and(|i| i.healthy),
        "{when}: build_poc_doc must count fryDVPN as healthy: {:?}",
        doc.integrations
    );
    assert_eq!(doc.proportion, 1.0, "{when}: the proportion");
    assert_eq!(
        doc.slots.first().map(|slot| slot.multiplier),
        Some(1.0),
        "{when}: fryDVPN is this device's only healthy integration, so its multiplier is 1.0"
    );
}

#[cfg(unix)]
#[test]
fn a_registered_node_short_at_start_reports_frynodes_health_to_poc() {
    run_scenario_on_frynode_port(module_path!(), "short_at_start");
}

#[cfg(unix)]
#[test]
fn a_registered_node_that_drains_while_running_reports_frynodes_health_to_poc() {
    run_scenario_on_frynode_port(module_path!(), "drains_while_running");
}

#[cfg(unix)]
#[test]
#[ignore = "scenario entry point: the tests above run it in a child process"]
fn child() {
    let Ok(scenario) = std::env::var(SCENARIO_VAR) else {
        return;
    };
    match scenario.as_str() {
        "short_at_start" => short_at_start(),
        "drains_while_running" => drains_while_running(),
        other => panic!("unknown scenario {other}"),
    }
    println!("{DONE} {scenario}");
}

/// 14 µALGO spendable: registered, and unable to pay one heartbeat — where
/// seven of the eleven live mainnet boxes sat (decisions D-20).
#[cfg(unix)]
fn short_at_start() {
    let s = Scene::new(
        World::new(100_014, RegistryBox::Present),
        Endpoint::PortInline,
        None,
    );
    install_decoy_frynode(&s);
    let _health = frynode_health_decoy();

    let started = s.block_on(s.integ.start());
    // The first report after the start, before anything else asks for health.
    assert_counted_healthy(
        &s,
        &format!(
            "short at start (start returned {started:?}, parked: {:?})",
            s.parked()
        ),
    );
    for check in 1..=12 {
        let status = s.block_on(s.integ.health_check());
        assert_eq!(
            status,
            HealthStatus::Healthy,
            "check {check}: a registered node short of one heartbeat fee must report frynode's \
             own health (start returned {started:?}, parked: {:?})",
            s.parked()
        );
    }
    assert_counted_healthy(&s, "short at start");
    s.assert_nothing_submitted();
}

/// Funded at start, then drained below one heartbeat fee by its own
/// heartbeats while running.
#[cfg(unix)]
fn drains_while_running() {
    let s = Scene::new(
        World::new(400_000, RegistryBox::Present),
        Endpoint::PortInline,
        None,
    );
    install_decoy_frynode(&s);
    let _health = frynode_health_decoy();

    let started = s.block_on(s.integ.start());
    assert!(started.is_ok(), "{started:?}");
    for check in 1..=3 {
        assert_eq!(
            s.block_on(s.integ.health_check()),
            HealthStatus::Healthy,
            "check {check}"
        );
    }
    s.set_world(|w| w.amount = 100_014);
    // Past two due re-reads (checks 7 and 15), so a drained wallet has been
    // seen by the time health is judged.
    for check in 4..=20 {
        assert_eq!(
            s.block_on(s.integ.health_check()),
            HealthStatus::Healthy,
            "check {check}: draining below one heartbeat fee must not change a running \
             registered node's health"
        );
    }
    assert_counted_healthy(&s, "drained while running");
    s.assert_nothing_submitted();
}
