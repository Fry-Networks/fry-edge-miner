//! D-C4-2 rework — the heartbeat shortfall of a registered node is a UI-only
//! notice: "send <fee + min-balance - amount> ALGO to <address>" in the
//! card's error line, never a health status.
//!
//! These read `funding_notice()`, which the rework introduced, so they cannot
//! be compiled against the tree before it. The health half of the same
//! contract is RED on 5b7c8f5 in `fryvpn_registered_health_tests`.

use super::fryvpn_c4_support::*;
use super::*;

#[test]
fn the_notice_is_set_at_start_and_dropped_by_a_stop() {
    run_scenario(module_path!(), "notice_at_start");
}

#[cfg(unix)]
#[test]
fn the_notice_clears_once_funded_without_a_restart() {
    run_scenario_on_frynode_port(module_path!(), "notice_clears_when_funded");
}

#[cfg(unix)]
#[test]
fn the_notice_appears_when_a_running_node_drains() {
    run_scenario_on_frynode_port(module_path!(), "notice_on_drain");
}

/// `get_integrations` needs a live Tauri app, so its one-line merge is pinned
/// on the source: the notice reaches `IntegrationStatus.error` for fryDVPN
/// only, ranked after a real start error and an elevation block, and nothing
/// that builds health or the PoC document ever reads it.
#[test]
fn the_card_error_carries_the_notice_for_fryvpn_only_and_health_never_reads_it() {
    let src = include_str!("../commands/integration.rs");
    let start = src
        .find("error: last_errors")
        .expect("get_integrations builds the card's error line");
    let end = start
        + src[start..]
            .find("unavailable_reason")
            .expect("the error expression ends before unavailable_reason");
    let expr = &src[start..end];
    let elevation = expr
        .find("elevation_blocks")
        .expect("the elevation-block merge is still there");
    let notice = expr.find("fryvpn::funding_notice");
    assert!(
        notice.is_some(),
        "the fryDVPN funding notice must reach the card's error line:\n{expr}"
    );
    let notice = notice.unwrap();
    assert!(
        elevation < notice,
        "a real start error and an elevation block outrank the notice:\n{expr}"
    );
    assert!(
        expr[elevation..notice].contains(r#"id == "fryvpn""#),
        "the notice belongs to fryDVPN's card only:\n{expr}"
    );
    assert_eq!(
        src.matches("funding_notice").count(),
        1,
        "commands/integration.rs reads the notice only for the error line"
    );
    for (name, file) in [
        ("poc/reporter.rs", include_str!("../poc/reporter.rs")),
        ("poc/gates.rs", include_str!("../poc/gates.rs")),
        (
            "supervisor/health.rs",
            include_str!("../supervisor/health.rs"),
        ),
        ("main.rs", include_str!("../main.rs")),
    ] {
        assert!(
            !file.contains("funding_notice"),
            "{name} must never read the funding notice: it is UI only"
        );
    }
}

#[test]
#[ignore = "scenario entry point: the tests above run it in a child process"]
fn child() {
    let Ok(scenario) = std::env::var(SCENARIO_VAR) else {
        return;
    };
    match scenario.as_str() {
        "notice_at_start" => notice_at_start(),
        #[cfg(unix)]
        "notice_clears_when_funded" => notice_clears_when_funded(),
        #[cfg(unix)]
        "notice_on_drain" => notice_on_drain(),
        other => panic!("unknown scenario {other}"),
    }
    println!("{DONE} {scenario}");
}

fn assert_is_the_shortfall_notice(notice: Option<String>, when: &str) {
    assert!(
        notice.is_some(),
        "{when}: a registered node that cannot pay its next heartbeat has no notice"
    );
    let notice = notice.unwrap();
    assert!(
        notice.contains(&format!("send 0.000986 ALGO to {ADDR}")),
        "{when}: the shortfall is the heartbeat fee plus the account minimum, less what it \
         holds (101_000 - 100_014 = 986 µALGO), sent to the node's own address: {notice}"
    );
    assert!(
        notice.contains("0.001 ALGO fee"),
        "{when}: names the call it cannot pay: {notice}"
    );
}

/// Every platform: the notice follows the measured wallet at start, and a
/// stop leaves nothing behind.
fn notice_at_start() {
    for (amount, short) in [(100_014, true), (110_000, false), (400_000, false)] {
        let s = Scene::new(
            World::new(amount, RegistryBox::Present),
            Endpoint::PortInline,
            None,
        );
        let started = s.block_on(s.integ.start());
        assert!(Scene::spawn_attempted(&started), "{amount}: {started:?}");
        assert_eq!(
            s.parked(),
            None,
            "{amount}: registered nodes are never parked"
        );
        if short {
            assert_is_the_shortfall_notice(funding_notice(), "at start");
        } else {
            assert_eq!(
                funding_notice(),
                None,
                "{amount} µALGO pays at least one heartbeat: no notice"
            );
        }
        let _ = s.block_on(s.integ.stop());
        assert_eq!(funding_notice(), None, "{amount}: a stop drops the notice");
    }
}

#[cfg(unix)]
fn frynode_pid(s: &Scene) -> Option<u32> {
    s.integ
        .supervisor
        .lock()
        .unwrap()
        .list_processes()
        .into_iter()
        .find(|p| p.integration_id == "fryvpn" && p.running)
        .map(|p| p.pid)
}

#[cfg(unix)]
fn notice_clears_when_funded() {
    let s = Scene::new(
        World::new(100_014, RegistryBox::Present),
        Endpoint::PortInline,
        None,
    );
    install_decoy_frynode(&s);
    let _health = frynode_health_decoy();

    let started = s.block_on(s.integ.start());
    assert!(started.is_ok(), "{started:?}");
    let pid = frynode_pid(&s).expect("the decoy frynode is running");
    assert_is_the_shortfall_notice(funding_notice(), "after start");

    for check in 1..=13 {
        assert_eq!(
            s.block_on(s.integ.health_check()),
            HealthStatus::Healthy,
            "check {check}: the notice never becomes health"
        );
        assert_is_the_shortfall_notice(funding_notice(), &format!("check {check}"));
    }

    s.set_world(|w| w.amount = 200_000);
    let cleared_at = (14..=24).find(|check| {
        assert_eq!(
            s.block_on(s.integ.health_check()),
            HealthStatus::Healthy,
            "check {check}"
        );
        funding_notice().is_none()
    });
    assert_eq!(
        cleared_at,
        Some(15),
        "the notice clears at the next due re-read after funding (check 15)"
    );
    assert_eq!(
        frynode_pid(&s),
        Some(pid),
        "the same frynode process is still running: nothing was restarted"
    );
    s.assert_nothing_submitted();
}

#[cfg(unix)]
fn notice_on_drain() {
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
        assert_eq!(s.block_on(s.integ.health_check()), HealthStatus::Healthy);
        assert_eq!(funding_notice(), None, "check {check}: funded");
    }
    s.set_world(|w| w.amount = 100_014);
    let noticed_at = (4..=14).find(|check| {
        assert_eq!(
            s.block_on(s.integ.health_check()),
            HealthStatus::Healthy,
            "check {check}: draining never changes health"
        );
        funding_notice().is_some()
    });
    assert_eq!(
        noticed_at,
        Some(7),
        "the drain is noticed at the next due re-read (check 7)"
    );
    assert_is_the_shortfall_notice(funding_notice(), "after the drain");
    s.assert_nothing_submitted();
}
