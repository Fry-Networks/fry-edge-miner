//! FAIL-2 (the narrow balance gate) and the fryDVPN fail-open, per operator
//! decision 1 and D-C4-2.
//!
//! - A node already registered on chain is never parked for its balance.
//! - The affordability check guards only what spends: registration, which
//!   FEM gates by whether it spawns an UNREGISTERED node. The registration
//!   price is still exact, re-reads back off within a bound, and the node
//!   resumes on its own once funded.
//! - A registered node that cannot pay its next heartbeat shows ONE funding
//!   state, logged once per state change, never per heartbeat or per check.
//!   Since the chunk-2 rework that state is a UI-only notice: health stays
//!   frynode's own.
//! - An unreadable balance spends nothing: an unregistered node is not
//!   spawned, and a registered one, or one whose registration could not be
//!   read either, stays up. A failed read is never taken as "registered".
//!
//! Each scenario runs in a child process; see `fryvpn_c4_support`.

use super::fryvpn_c4_support::*;
use super::*;
use crate::supervisor::health::{recovery_action, RecoveryAction};

/// The one log line a registered node gets when it enters the heartbeat
/// shortfall state, and the one it gets when it leaves it.
const SHORTFALL_ENTERED: &str = "cannot pay its next heartbeat";
#[cfg(unix)]
const SHORTFALL_LEFT: &str = "can pay its heartbeats again";

#[test]
fn a_registered_underfunded_node_is_not_parked() {
    run_scenario(module_path!(), "registered_underfunded");
}

#[test]
fn a_registered_node_that_cannot_pay_a_heartbeat_still_starts_and_logs_one_shortfall() {
    run_scenario(module_path!(), "registered_below_heartbeat_fee");
}

/// Needs a frynode stand-in that stays alive under frynode's own flags, which
/// only a script provides; the start-path half of D-C4-2 is covered on every
/// platform by the test above.
#[cfg(unix)]
#[test]
fn a_running_registered_node_shows_one_shortfall_state_until_funded() {
    run_scenario_on_frynode_port(module_path!(), "registered_short_running");
}

#[test]
fn an_unregistered_underfunded_node_is_not_spawned_and_rereads_on_a_bounded_backoff() {
    run_scenario_on_frynode_port(module_path!(), "unregistered_underfunded");
}

#[test]
fn an_unreadable_balance_never_spawns_an_unregistered_node() {
    run_scenario(module_path!(), "unreadable_unregistered");
}

#[test]
fn an_unreadable_balance_keeps_a_registered_node_up() {
    run_scenario(module_path!(), "unreadable_registered");
}

#[test]
fn an_unreadable_balance_with_an_unreadable_registry_keeps_the_node_up() {
    run_scenario(module_path!(), "unreadable_unknown");
}

#[test]
fn a_failed_registry_read_is_never_taken_as_registered() {
    run_scenario(module_path!(), "underfunded_unknown");
}

#[test]
fn a_funded_wallet_starts_without_any_registry_read() {
    run_scenario(module_path!(), "funded");
}

#[test]
fn the_registry_read_goes_through_the_algod_overrides() {
    run_scenario(module_path!(), "box_through_overrides");
}

#[test]
#[ignore = "scenario entry point: the tests above run it in a child process"]
fn child() {
    let Ok(scenario) = std::env::var(SCENARIO_VAR) else {
        return;
    };
    match scenario.as_str() {
        "registered_underfunded" => registered_underfunded(),
        "registered_below_heartbeat_fee" => registered_below_heartbeat_fee(),
        #[cfg(unix)]
        "registered_short_running" => registered_short_running(),
        "unregistered_underfunded" => unregistered_underfunded(),
        "unreadable_unregistered" => unreadable_unregistered(),
        "unreadable_registered" => unreadable_registered(),
        "unreadable_unknown" => unreadable_unknown(),
        "underfunded_unknown" => underfunded_unknown(),
        "funded" => funded(),
        "box_through_overrides" => box_through_overrides(),
        other => panic!("unknown scenario {other}"),
    }
    println!("{DONE} {scenario}");
}

fn registration_message(amount: u64) -> String {
    registration_funding_message(amount, MIN_BALANCE, REGISTRATION_MIN_MICROALGOS, ADDR)
        .expect_err("fixture: this amount cannot afford registration")
}

fn reason(status: &HealthStatus) -> &str {
    match status {
        HealthStatus::Unhealthy(r) => r,
        _ => "",
    }
}

/// The post-registration state measured on LocalNet: a node funded to exactly
/// what the card asked for registers and is left at 110_000 / 100_000, i.e.
/// 10_000 spendable — well below the 212_000 registration price.
fn registered_underfunded() {
    let s = Scene::new(
        World::new(110_000, RegistryBox::Present),
        Endpoint::PortInline,
        None,
    );
    let started = s.block_on(s.integ.start());
    assert_eq!(
        s.parked(),
        None,
        "FAIL-2: a node already registered on chain was parked behind the registration price \
         (start returned {started:?})"
    );
    assert!(
        Scene::spawn_attempted(&started),
        "the frynode spawn path must run for a registered node: {started:?}"
    );
    assert_eq!(
        s.reads(&box_path()),
        1,
        "registration comes from one registry-box read"
    );
    assert!(
        s.log_lines(SHORTFALL_ENTERED).is_empty(),
        "10_000 spendable pays ten heartbeats; there is no heartbeat shortfall"
    );
    assert_eq!(
        s.block_on(s.integ.resolve_funded_identity()),
        Ok(Some(mnemonic())),
        "a registered node must be started WITH the device identity, or frynode signs as a \
         different, unregistered account"
    );
    s.assert_nothing_submitted();
}

/// Seven of the eleven live mainnet registered boxes sat at exactly 14 µALGO
/// spendable (decisions D-20): registered, and unable to pay one heartbeat.
fn registered_below_heartbeat_fee() {
    let s = Scene::new(
        World::new(100_014, RegistryBox::Present),
        Endpoint::PortInline,
        None,
    );
    let started = s.block_on(s.integ.start());
    assert_eq!(
        s.parked(),
        None,
        "D-C4-2: a registered node starts whatever its balance (start returned {started:?})"
    );
    assert!(Scene::spawn_attempted(&started), "{started:?}");

    let entered = s.log_lines(SHORTFALL_ENTERED);
    assert_eq!(
        entered.len(),
        1,
        "the heartbeat shortfall is logged exactly once when the state is entered: {entered:?}"
    );
    assert!(
        entered[0].contains(&format!("send 0.000986 ALGO to {ADDR}")),
        "the shortfall is the heartbeat fee plus the account minimum, less what it holds \
         (101_000 - 100_014 = 986 µALGO), sent to the node's own address: {}",
        entered[0]
    );

    // Measuring the same wallet again is not a state change.
    assert_eq!(
        s.block_on(s.integ.resolve_funded_identity()),
        Ok(Some(mnemonic()))
    );
    assert_eq!(
        s.log_lines(SHORTFALL_ENTERED).len(),
        1,
        "a repeat measurement must not log again"
    );
    s.assert_nothing_submitted();
}

/// D-C4-2 end to end on a live process: one state while short, re-read on the
/// bounded backoff, cleared by funding with no restart, logged once per change,
/// and never masking a node that has died.
#[cfg(unix)]
fn registered_short_running() {
    let s = Scene::new(
        World::new(100_014, RegistryBox::Present),
        Endpoint::PortInline,
        None,
    );
    install_decoy_frynode(&s);
    let health = frynode_health_decoy();

    let started = s.block_on(s.integ.start());
    assert_eq!(
        s.parked(),
        None,
        "D-C4-2: a registered node is started, never parked (start returned {started:?})"
    );
    assert!(started.is_ok(), "the decoy frynode must spawn: {started:?}");
    assert!(
        matches!(
            s.integ.supervisor.lock().unwrap().get_status("fryvpn"),
            HealthStatus::Healthy
        ),
        "frynode must be running"
    );

    // D-C4-2 (chunk-2 rework): the balance never reaches health. A registered
    // node reports frynode's own health, and the shortfall lives in a UI-only
    // notice — observed here through its once-per-change log lines.
    for check in 1..=13 {
        assert_eq!(
            s.block_on(s.integ.health_check()),
            HealthStatus::Healthy,
            "check {check}: a registered node's health is frynode's own, whatever its balance"
        );
    }
    let entered = s.log_lines(SHORTFALL_ENTERED);
    assert_eq!(
        entered.len(),
        1,
        "logged once on entering the state, not per check or per heartbeat: {entered:?}"
    );
    assert!(
        entered[0].contains(&format!("send 0.000986 ALGO to {ADDR}")),
        "{}",
        entered[0]
    );
    assert_eq!(
        s.reads(&account_path()),
        4,
        "the start's read plus re-reads at checks 1, 3 and 7 only (bounded backoff)"
    );

    // Funded: the state clears at the next due re-read, with no restart.
    s.set_world(|w| w.amount = 200_000);
    let cleared_at = (14..=24).find(|check| {
        assert_eq!(
            s.block_on(s.integ.health_check()),
            HealthStatus::Healthy,
            "check {check}"
        );
        s.log_lines(SHORTFALL_LEFT).len() == 1
    });
    assert_eq!(
        cleared_at,
        Some(15),
        "the shortfall clears at the next due re-read (check 15), logged once"
    );
    assert!(
        matches!(
            s.integ.supervisor.lock().unwrap().get_status("fryvpn"),
            HealthStatus::Healthy
        ),
        "the same frynode is still running: nothing was restarted"
    );

    // Drained again by its heartbeats: a second state change, logged once more.
    s.set_world(|w| w.amount = 100_014);
    let short_again_at = (16..=25).find(|check| {
        assert_eq!(
            s.block_on(s.integ.health_check()),
            HealthStatus::Healthy,
            "check {check}"
        );
        s.log_lines(SHORTFALL_ENTERED).len() == 2
    });
    assert_eq!(
        short_again_at,
        Some(25),
        "re-detected at the next due re-read"
    );

    // A start on the already-running node (boot pass, a toggle) while algod
    // is unreadable proves nothing: the state stays, and nothing is logged.
    s.set_world(|w| w.account = Account::Html503);
    let again = s.block_on(s.integ.start());
    assert!(again.is_ok(), "{again:?}");
    assert_eq!(s.block_on(s.integ.health_check()), HealthStatus::Healthy);
    assert_eq!(
        s.log_lines(SHORTFALL_LEFT).len(),
        1,
        "a failed read is not a state change"
    );
    s.set_world(|w| w.account = Account::Balance);

    // A dead node is a fault to restart, never hidden behind the funding state.
    drop(health);
    s.integ
        .supervisor
        .lock()
        .unwrap()
        .stop_integration("fryvpn")
        .expect("stop the decoy frynode");
    let dead = s.block_on(s.integ.health_check());
    assert!(
        matches!(dead, HealthStatus::Unhealthy(_)) && !reason(&dead).starts_with(FUNDING_MARKER),
        "a frynode that is not running must be reported as such, so the supervisor restarts \
         it: {dead:?}"
    );

    // Nothing about that wallet survives a stop: a node started next WITHOUT
    // the device identity (hardwareapi withholds the mnemonic) is not judged
    // by the old wallet's balance.
    let _ = s.block_on(s.integ.stop());
    s.set_world(|w| w.mnemonic_released = false);
    let health = frynode_health_decoy();
    let restarted = s.block_on(s.integ.start());
    assert!(restarted.is_ok(), "{restarted:?}");
    let reads_before = s.reads(&account_path());
    for check in 1..=3 {
        assert_eq!(
            s.block_on(s.integ.health_check()),
            HealthStatus::Healthy,
            "check {check}: a node started without an identity reports frynode's own health"
        );
    }
    assert_eq!(
        s.reads(&account_path()),
        reads_before,
        "no wallet is read for a node FEM holds no identity for"
    );
    drop(health);
    s.assert_nothing_submitted();
}

fn unregistered_underfunded() {
    let s = Scene::new(
        World::new(110_000, RegistryBox::Absent),
        Endpoint::PortInline,
        None,
    );
    let started = s.block_on(s.integ.start());
    let expected = registration_message(110_000);
    assert_eq!(
        s.parked(),
        Some(expected.clone()),
        "an unregistered node that cannot afford registration is parked with the exact \
         registration shortfall (start returned {started:?})"
    );
    assert!(started.is_ok(), "parked, frynode not spawned: {started:?}");
    assert!(expected.contains(&format!("send 0.202 ALGO to {ADDR}")));

    let mut reread_at = Vec::new();
    for check in 1..=35 {
        let before = s.reads(&account_path());
        let status = s.block_on(s.integ.health_check());
        assert_eq!(
            status,
            HealthStatus::Unhealthy(expected.clone()),
            "check {check}"
        );
        if s.reads(&account_path()) > before {
            reread_at.push(check);
        }
    }
    assert_eq!(
        reread_at,
        vec![1, 3, 7, 15, 25, 35],
        "FAIL-2: re-reads back off (gaps 1, 2, 4, 8, then at most 10 checks = 5 min at the 30 s \
         health interval) instead of reading algod on every check"
    );
    assert!(
        s.reads(&box_path()) >= 1,
        "the registration verdict came from the registry box"
    );

    // Funded: the park lifts at the next due re-read, with no user action.
    require_frynode_port_free();
    s.set_world(|w| w.amount = 400_000);
    let resumed_at = (36..=45).find(|_| {
        let _ = s.block_on(s.integ.health_check());
        s.parked().is_none()
    });
    assert_eq!(
        resumed_at,
        Some(45),
        "auto-resume at the next due re-read, within the backoff bound"
    );
    let after = s.block_on(s.integ.health_check());
    assert!(
        !reason(&after).starts_with(FUNDING_MARKER),
        "un-parked, the not-running process is a restartable fault: {after:?}"
    );
    assert_eq!(recovery_action(&after, true, 0, 6), RecoveryAction::Restart);
    // The supervisor's restart runs start(), which now spawns.
    let restarted = s.block_on(s.integ.start());
    assert!(Scene::spawn_attempted(&restarted), "{restarted:?}");
    s.assert_nothing_submitted();
}

fn unreadable_unregistered() {
    let s = Scene::new(
        World {
            account: Account::Html503,
            ..World::new(0, RegistryBox::Absent)
        },
        Endpoint::PortInline,
        None,
    );
    let started = s.block_on(s.integ.start());
    let expected = balance_unreadable_message("algod returned HTTP 503 Service Unavailable");
    assert_eq!(
        s.parked(),
        Some(expected.clone()),
        "fail-open: with the balance unreadable and the node NOT registered, spawning frynode \
         would let it attempt a registration nobody priced — it must stay parked (start \
         returned {started:?})"
    );
    assert!(started.is_ok(), "{started:?}");
    let status = s.block_on(s.integ.health_check());
    assert_eq!(
        status,
        HealthStatus::Unhealthy(expected),
        "still parked, quoting no figure"
    );
    s.assert_nothing_submitted();
}

fn unreadable_registered() {
    let s = Scene::new(
        World {
            account: Account::Html503,
            ..World::new(0, RegistryBox::Present)
        },
        Endpoint::PortInline,
        None,
    );
    let started = s.block_on(s.integ.start());
    assert_eq!(
        s.parked(),
        None,
        "availability: a registered node stays up when its balance cannot be read"
    );
    assert!(Scene::spawn_attempted(&started), "{started:?}");
    assert_eq!(
        s.block_on(s.integ.resolve_funded_identity()),
        Ok(Some(mnemonic()))
    );
    s.assert_nothing_submitted();
}

fn unreadable_unknown() {
    for registry_box in [
        RegistryBox::Html503,
        RegistryBox::Html404,
        RegistryBox::Dropped,
    ] {
        let s = Scene::new(
            World {
                account: Account::Html503,
                ..World::new(0, registry_box)
            },
            Endpoint::PortInline,
            None,
        );
        let started = s.block_on(s.integ.start());
        assert_eq!(
            s.parked(),
            None,
            "{registry_box:?}: availability — with neither read conclusive the node stays up; \
             only algod's own \"box not found\" proves it unregistered"
        );
        assert!(
            Scene::spawn_attempted(&started),
            "{registry_box:?}: {started:?}"
        );
        s.assert_nothing_submitted();
    }
}

fn underfunded_unknown() {
    for registry_box in [
        RegistryBox::Html200,
        RegistryBox::JsonWithoutValue,
        RegistryBox::Html503,
        RegistryBox::Html404,
        RegistryBox::Dropped,
    ] {
        let s = Scene::new(
            World::new(110_000, registry_box),
            Endpoint::PortInline,
            None,
        );
        let started = s.block_on(s.integ.start());
        assert_eq!(
            s.parked(),
            Some(registration_message(110_000)),
            "{registry_box:?}: a registry read that failed is never taken as registered — the \
             registration gate stands (start returned {started:?})"
        );
        assert!(started.is_ok(), "{registry_box:?}: {started:?}");
        assert!(s.reads(&box_path()) <= 1, "{registry_box:?}");
        s.assert_nothing_submitted();
    }
}

fn funded() {
    let s = Scene::new(
        World::new(400_000, RegistryBox::Present),
        Endpoint::PortInline,
        None,
    );
    let started = s.block_on(s.integ.start());
    assert_eq!(s.parked(), None, "{started:?}");
    assert!(Scene::spawn_attempted(&started), "{started:?}");
    assert_eq!(
        s.reads(&box_path()),
        0,
        "a wallet that can afford registration costs no extra algod read"
    );
    s.assert_nothing_submitted();
}

fn box_through_overrides() {
    let s = Scene::new(
        World {
            require_token: true,
            ..World::new(110_000, RegistryBox::Present)
        },
        Endpoint::PortOverride,
        Some(TOKEN),
    );
    let started = s.block_on(s.integ.start());
    assert!(Scene::spawn_attempted(&started), "{started:?}");
    let reads = s.algod.requests_to(&box_path());
    assert_eq!(
        reads.len(),
        1,
        "the registry read must use the same algod endpoint, port and token as the balance read"
    );
    assert_eq!(reads[0].header("X-Algo-API-Token"), Some(TOKEN));
    assert_eq!(s.parked(), None);
    s.assert_nothing_submitted();
}
