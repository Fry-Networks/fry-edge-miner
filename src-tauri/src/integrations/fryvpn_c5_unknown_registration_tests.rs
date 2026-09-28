//! Continuation #5, D-C5-6 (variant C): an Unknown registration — the
//! NodeRegistry box read failed — never changes a node's state.
//!
//! - A node that has not started waits for a successful read: it is not
//!   spawned and gets no park notice, and the supervisor does not restart it.
//! - A node parked before the read failed keeps its park and its notice.
//! - A running node keeps running.
//! - While FEM's own registry read is Unknown, frynode reporting itself
//!   unregistered is Unknown too, never a reason to restart it.
//!
//! Each scenario runs in a child process; see `fryvpn_c4_support`.

use super::fryvpn_c4_support::*;
use super::*;
use crate::supervisor::health::{recovery_action, RecoveryAction};

/// Every registry answer that proves nothing (`registration_from_box_read`).
const UNKNOWN_BOXES: [RegistryBox; 5] = [
    RegistryBox::Html200,
    RegistryBox::JsonWithoutValue,
    RegistryBox::Html503,
    RegistryBox::Html404,
    RegistryBox::Dropped,
];

#[test]
fn a_node_that_has_not_started_waits_for_a_registry_read_that_works() {
    run_scenario(module_path!(), "unstarted_waits");
}

/// Reaches the not-running branch, which probes frynode's API port.
#[test]
fn a_waiting_node_follows_the_first_registry_read_that_works() {
    run_scenario_on_frynode_port(module_path!(), "wait_then_read");
}

#[test]
fn a_parked_node_keeps_its_park_and_its_notice_while_the_registry_read_fails() {
    run_scenario(module_path!(), "parked_keeps_park");
}

/// Needs a frynode stand-in that stays alive, which only a script provides.
#[cfg(unix)]
#[test]
fn a_running_node_keeps_running_when_a_start_meets_an_unknown_registration() {
    run_scenario_on_frynode_port(module_path!(), "running_keeps_running");
}

#[cfg(unix)]
#[test]
fn frynode_reporting_itself_unregistered_is_not_restarted_while_the_registry_read_is_unknown() {
    run_scenario_on_frynode_port(module_path!(), "unregistered_report_unknown");
}

#[test]
#[ignore = "scenario entry point: the tests above run it in a child process"]
fn child() {
    let Ok(scenario) = std::env::var(SCENARIO_VAR) else {
        return;
    };
    match scenario.as_str() {
        "unstarted_waits" => unstarted_waits(),
        "wait_then_read" => wait_then_read(),
        "parked_keeps_park" => parked_keeps_park(),
        #[cfg(unix)]
        "running_keeps_running" => running_keeps_running(),
        #[cfg(unix)]
        "unregistered_report_unknown" => unregistered_report_unknown(),
        other => panic!("unknown scenario {other}"),
    }
    println!("{DONE} {scenario}");
}

fn registration_message(amount: u64) -> String {
    registration_funding_message(amount, MIN_BALANCE, REGISTRATION_MIN_MICROALGOS, ADDR)
        .expect_err("fixture: this amount cannot afford registration")
}

/// The supervisor's own verdict on a status, for an enabled integration.
fn supervisor_action(status: &HealthStatus) -> RecoveryAction {
    recovery_action(status, true, 0, 6)
}

fn unstarted_waits() {
    for registry_box in UNKNOWN_BOXES {
        let s = Scene::new(
            World::new(110_000, registry_box),
            Endpoint::PortInline,
            None,
        );
        let started = s.block_on(s.integ.start());
        assert_eq!(
            s.parked(),
            None,
            "{registry_box:?}: D-C5-6 variant C — a failed registry read never parks a node that \
             has not started (start returned {started:?})"
        );
        assert!(
            started.is_ok() && !Scene::spawn_attempted(&started),
            "{registry_box:?}: the node waits, it is not spawned: {started:?}"
        );
        for check in 1..=3 {
            let status = s.block_on(s.integ.health_check());
            assert_eq!(
                status,
                HealthStatus::Unknown,
                "{registry_box:?}, check {check}: waiting on the registry is Unknown, with no \
                 notice on the card"
            );
            assert_eq!(
                supervisor_action(&status),
                RecoveryAction::None,
                "{registry_box:?}: the supervisor must not restart a waiting node"
            );
        }
        assert_eq!(s.parked(), None, "{registry_box:?}");
        assert!(
            s.reads(&box_path()) >= 2,
            "{registry_box:?}: the wait keeps reading the registry box"
        );
        s.assert_nothing_submitted();
    }
}

fn wait_then_read() {
    // The read comes back REGISTERED: the wait lifts and the ordinary restart
    // starts the node.
    let s = Scene::new(
        World::new(110_000, RegistryBox::Html503),
        Endpoint::PortInline,
        None,
    );
    let started = s.block_on(s.integ.start());
    assert!(
        started.is_ok() && !Scene::spawn_attempted(&started),
        "waits: {started:?}"
    );
    assert_eq!(s.parked(), None, "a wait, not a park");
    s.set_world(|w| w.registry_box = RegistryBox::Present);
    require_frynode_port_free();
    let status = s.block_on(s.integ.health_check());
    assert!(
        matches!(&status, HealthStatus::Unhealthy(r) if r.contains("frynode process is not running")),
        "a registered read ends the wait; the node is simply not running yet: {status:?}"
    );
    assert_eq!(supervisor_action(&status), RecoveryAction::Restart);
    let restarted = s.block_on(s.integ.start());
    assert!(
        Scene::spawn_attempted(&restarted),
        "the restart spawns the registered node: {restarted:?}"
    );
    assert_eq!(s.parked(), None);
    s.assert_nothing_submitted();
    drop(s);

    // The read comes back UNREGISTERED: the wait becomes the ordinary park,
    // with the exact registration shortfall.
    let s = Scene::new(
        World::new(110_000, RegistryBox::Html503),
        Endpoint::PortInline,
        None,
    );
    let started = s.block_on(s.integ.start());
    assert!(started.is_ok() && !Scene::spawn_attempted(&started));
    assert_eq!(s.parked(), None, "a wait, not a park");
    s.set_world(|w| w.registry_box = RegistryBox::Absent);
    let expected = registration_message(110_000);
    assert_eq!(
        s.block_on(s.integ.health_check()),
        HealthStatus::Unhealthy(expected.clone())
    );
    assert_eq!(s.parked(), Some(expected));
    s.assert_nothing_submitted();
    drop(s);

    // A stop forgets the wait, as it forgets everything learned about the
    // wallet: a later check reports the node as it is, not as waiting.
    let s = Scene::new(
        World::new(110_000, RegistryBox::Html503),
        Endpoint::PortInline,
        None,
    );
    let started = s.block_on(s.integ.start());
    assert!(started.is_ok() && !Scene::spawn_attempted(&started));
    assert_eq!(s.block_on(s.integ.health_check()), HealthStatus::Unknown);
    s.block_on(s.integ.stop()).expect("stop");
    let after_stop = s.block_on(s.integ.health_check());
    assert!(
        matches!(&after_stop, HealthStatus::Unhealthy(r) if r.contains("frynode process is not running")),
        "a stop must not leave the node waiting: {after_stop:?}"
    );
}

fn parked_keeps_park() {
    let s = Scene::new(
        World::new(110_000, RegistryBox::Absent),
        Endpoint::PortInline,
        None,
    );
    let started = s.block_on(s.integ.start());
    let notice = registration_message(110_000);
    assert_eq!(s.parked(), Some(notice.clone()), "{started:?}");

    // The registry read now fails, and the wallet moves. With nothing proven
    // about registration, neither the park nor its notice may change.
    s.set_world(|w| {
        w.registry_box = RegistryBox::Html503;
        w.amount = 150_000;
    });
    let box_reads_before = s.reads(&box_path());
    for check in 1..=10 {
        assert_eq!(
            s.block_on(s.integ.health_check()),
            HealthStatus::Unhealthy(notice.clone()),
            "check {check}: D-C5-6 variant C — the park and its notice stay exactly as they were"
        );
        assert_eq!(s.parked(), Some(notice.clone()), "check {check}");
    }
    assert!(
        s.reads(&box_path()) > box_reads_before,
        "the registry was re-read and failed; the park did not simply skip the read"
    );
    s.assert_nothing_submitted();
}

#[cfg(unix)]
fn running_keeps_running() {
    let s = Scene::new(
        World::new(110_000, RegistryBox::Present),
        Endpoint::PortInline,
        None,
    );
    install_decoy_frynode(&s);
    let _health = frynode_health_decoy();
    let started = s.block_on(s.integ.start());
    assert!(started.is_ok(), "the decoy frynode must spawn: {started:?}");
    assert_eq!(s.block_on(s.integ.health_check()), HealthStatus::Healthy);

    // A start on the running node (boot pass, a toggle) while the registry
    // read fails.
    s.set_world(|w| w.registry_box = RegistryBox::Html503);
    let again = s.block_on(s.integ.start());
    assert!(again.is_ok(), "{again:?}");
    assert_eq!(
        s.parked(),
        None,
        "D-C5-6 variant C: a running node is never parked over a failed registry read"
    );
    assert!(
        matches!(
            s.integ.supervisor.lock().unwrap().get_status("fryvpn"),
            HealthStatus::Healthy
        ),
        "frynode keeps running"
    );
    assert_eq!(s.block_on(s.integ.health_check()), HealthStatus::Healthy);
    s.assert_nothing_submitted();
}

/// frynode's `/health`, alive but not registered — what a node reports when its
/// own registry read failed at start.
#[cfg(unix)]
fn frynode_unregistered_decoy() -> Decoy {
    let route = |req: &Request| {
        if req.target == "/health" {
            Reply::Http(
                200,
                "application/json",
                r#"{"status":"healthy","registered":false}"#.to_string(),
            )
        } else {
            Reply::Http(404, "application/json", "{}".to_string())
        }
    };
    for _ in 0..50 {
        if let Ok(decoy) = Decoy::serve(&format!("127.0.0.1:{FRYNODE_API_PORT}"), route) {
            return decoy;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("harness: port {FRYNODE_API_PORT} stayed busy");
}

#[cfg(unix)]
fn unregistered_report_unknown() {
    // Balance and registry both unreadable: the node is started (D-C4-9), and
    // frynode, whose own registry read fails the same way, reports itself
    // unregistered.
    let s = Scene::new(
        World {
            account: Account::Html503,
            ..World::new(0, RegistryBox::Html503)
        },
        Endpoint::PortInline,
        None,
    );
    install_decoy_frynode(&s);
    let health = frynode_unregistered_decoy();
    let started = s.block_on(s.integ.start());
    assert!(started.is_ok(), "{started:?}");
    assert_eq!(s.parked(), None);
    for check in 1..=3 {
        let status = s.block_on(s.integ.health_check());
        assert_eq!(
            status,
            HealthStatus::Unknown,
            "check {check}: with FEM's own registry read Unknown, frynode's registered=false \
             proves nothing"
        );
        assert_eq!(
            supervisor_action(&status),
            RecoveryAction::None,
            "check {check}: no restart churn"
        );
    }
    s.assert_nothing_submitted();
    drop(health);
    drop(s);

    // Control: FEM's own read says REGISTERED, frynode says it is not. That is
    // a real fault and keeps its restart.
    let s = Scene::new(
        World::new(110_000, RegistryBox::Present),
        Endpoint::PortInline,
        None,
    );
    install_decoy_frynode(&s);
    let _health = frynode_unregistered_decoy();
    let started = s.block_on(s.integ.start());
    assert!(started.is_ok(), "{started:?}");
    let status = s.block_on(s.integ.health_check());
    assert_eq!(
        status,
        HealthStatus::Unhealthy("dVPN not registered on-chain".to_string())
    );
    assert_eq!(supervisor_action(&status), RecoveryAction::Restart);
    s.assert_nothing_submitted();
}
