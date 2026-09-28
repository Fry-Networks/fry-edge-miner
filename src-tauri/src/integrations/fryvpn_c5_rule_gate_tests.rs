//! Continuation #5, D-C5-2: FEM never spawns frynode while Windows Firewall
//! has no enabled inbound allow rule for the installed frynode.exe.
//!
//! - fryDVPN then waits in a setup state — never a funding park — whose card
//!   line is the exact D-C5-2 instruction, marked so the supervisor does not
//!   restart it and the card reads "Setup required".
//! - An unregistered node short of the registration price shows that
//!   shortfall after the instruction, on the same line; a registered node's
//!   heartbeat notice is not appended (it describes a running node).
//! - The rule is re-checked on every health check; the check that finds it
//!   lets the supervisor's ordinary restart start frynode.
//! - Enabling fryDVPN raises no UAC prompt: the rule comes only from the
//!   Security hardening banner, which the setup state publishes.
//! - The rule gates spawning only: a running node keeps running.
//!
//! Windows is replaced by `FRYNODE_RULE_LISTING_FOR_TEST`: each scenario hands
//! the start path the listing Windows would print. Each scenario runs in a
//! child process; see `fryvpn_c4_support`.

use super::fryvpn_c4_support::*;
use super::*;
use crate::supervisor::health::{recovery_action, RecoveryAction};

/// The exact card line (D-C5-2 text after the "Awaiting administrator action"
/// marker that `integrations::AWAITING_MARKERS` already holds).
const SETUP: &str = "Awaiting administrator action — Click Retry on the Security hardening \
                     banner to allow fryDVPN through Windows Firewall.";

#[test]
fn a_missing_rule_never_spawns_frynode_and_shows_the_setup_text() {
    run_scenario(module_path!(), "missing");
}

#[test]
fn a_disabled_rule_never_spawns_frynode() {
    run_scenario(module_path!(), "disabled");
}

#[test]
fn a_rule_for_another_program_never_spawns_frynode() {
    run_scenario(module_path!(), "wrong_program");
}

#[test]
fn an_unreadable_rule_listing_never_spawns_frynode() {
    run_scenario(module_path!(), "unreadable");
}

#[test]
fn a_valid_rule_spawns_frynode() {
    run_scenario(module_path!(), "valid");
}

#[test]
fn a_registered_node_without_the_rule_waits_in_setup_not_in_a_funding_park() {
    run_scenario(module_path!(), "registered_without_rule");
}

#[test]
fn a_missing_rule_and_a_short_wallet_show_the_setup_text_then_the_shortfall() {
    run_scenario(module_path!(), "short_without_rule");
}

/// Reaches the not-running branch, which probes frynode's API port.
#[test]
fn the_rule_is_rechecked_every_tick_and_frynode_starts_when_it_appears() {
    run_scenario_on_frynode_port(module_path!(), "rule_appears");
}

/// Hands the start path an ABSOLUTE frynode path, the shape that reached the
/// old per-toggle firewall elevation; off Windows nothing can elevate.
#[cfg(not(windows))]
#[test]
fn enabling_fryvpn_raises_no_elevation_and_publishes_the_hardening_banner() {
    run_scenario(module_path!(), "toggle_no_elevation");
}

/// Needs a frynode stand-in that stays alive, which only a script provides.
#[cfg(unix)]
#[test]
fn a_running_node_keeps_running_when_its_rule_disappears() {
    run_scenario_on_frynode_port(module_path!(), "running_rule_gone");
}

/// The wallet read can take up to ~40 s; the rule is read after it, right
/// before the spawn, so a rule that disappears meanwhile still stops it.
#[test]
fn the_rule_is_read_after_the_wallet_right_before_the_spawn() {
    run_scenario(module_path!(), "rule_gone_during_wallet_read");
}

/// Variant C: entering the setup state does not wipe a funding park and its
/// notice that were already there.
#[test]
fn entering_the_setup_state_keeps_an_existing_funding_park() {
    run_scenario(module_path!(), "keeps_funding_park");
}

/// Entering the setup state asks the elevation gate for the hardening banner;
/// like the old per-start rule step, that runs off the async task.
#[test]
fn the_setup_state_is_entered_off_the_async_task() {
    let code: String = include_str!("fryvpn.rs")
        .lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let calls: Vec<usize> = code
        .match_indices("park_for_firewall_rule(")
        .map(|(at, _)| at)
        .filter(|&at| !code[..at].ends_with("fn "))
        .collect();
    assert!(
        !calls.is_empty(),
        "control: the setup state is entered somewhere"
    );
    for at in calls {
        let window = &code[at.saturating_sub(120)..at];
        assert!(
            window.contains("spawn_blocking(") || window.contains("block_in_place("),
            "park_for_firewall_rule runs on the async task at byte {at}: {window}"
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
        "missing" => refused(Some("NET|Public\r\nEND\r\n".to_string())),
        "disabled" => refused(Some(rule_listing_for(
            "Inbound|False|Allow|Any",
            &frynode(),
        ))),
        "wrong_program" => refused(Some(rule_listing_for(
            "Inbound|True|Allow|Any",
            r"C:\Program Files\Elsewhere\frynode.exe",
        ))),
        "unreadable" => refused(None),
        "valid" => valid(),
        "registered_without_rule" => registered_without_rule(),
        "short_without_rule" => short_without_rule(),
        "rule_appears" => rule_appears(),
        "rule_gone_during_wallet_read" => rule_gone_during_wallet_read(),
        "keeps_funding_park" => keeps_funding_park(),
        #[cfg(not(windows))]
        "toggle_no_elevation" => toggle_no_elevation(),
        #[cfg(unix)]
        "running_rule_gone" => running_rule_gone(),
        other => panic!("unknown scenario {other}"),
    }
    println!("{DONE} {scenario}");
}

/// The frynode the start path resolved — the program the rule must name.
fn frynode() -> String {
    FryVpnIntegration::binary_path().expect("the scenario's frynode")
}

fn rule_listing_for(fields: &str, program: &str) -> String {
    format!("RULE|{fields}|{program}\r\nNET|Public\r\nEND\r\n")
}

fn set_rule(listing: Option<String>) {
    *FRYNODE_RULE_LISTING_FOR_TEST.lock().unwrap() = Some(listing);
}

fn valid_rule() -> Option<String> {
    Some(rule_listing_for("Inbound|True|Allow|Any", &frynode()))
}

fn missing_rule() -> Option<String> {
    Some("NET|Public\r\nEND\r\n".to_string())
}

fn registration_message(amount: u64) -> String {
    registration_funding_message(amount, MIN_BALANCE, REGISTRATION_MIN_MICROALGOS, ADDR)
        .expect_err("fixture: this amount cannot afford registration")
}

fn assert_setup_state(s: &Scene, expected: &str, context: &str) {
    let status = s.block_on(s.integ.health_check());
    assert_eq!(
        status,
        HealthStatus::Unhealthy(expected.to_string()),
        "{context}: the card shows the D-C5-2 setup line"
    );
    assert!(
        crate::integrations::awaits_user_action(expected),
        "{context}: the line must carry a setup marker, or the card turns red"
    );
    assert_eq!(
        recovery_action(&status, true, 0, 6),
        RecoveryAction::None,
        "{context}: the supervisor must not restart a node waiting on the user"
    );
    assert_eq!(s.parked(), None, "{context}: this is not a funding park");
}

fn refused(listing: Option<String>) {
    let s = Scene::new(
        World::new(400_000, RegistryBox::Present),
        Endpoint::PortInline,
        None,
    );
    set_rule(listing);
    let started = s.block_on(s.integ.start());
    assert!(
        started.is_ok() && !Scene::spawn_attempted(&started),
        "D-C5-2: frynode must not be spawned without its firewall rule: {started:?}"
    );
    for check in 1..=3 {
        assert_setup_state(&s, SETUP, &format!("check {check}"));
    }
    s.assert_nothing_submitted();
}

fn valid() {
    let s = Scene::new(
        World::new(400_000, RegistryBox::Present),
        Endpoint::PortInline,
        None,
    );
    set_rule(valid_rule());
    let started = s.block_on(s.integ.start());
    assert!(
        Scene::spawn_attempted(&started),
        "a rule that admits frynode lets the start through: {started:?}"
    );
    assert_eq!(s.parked(), None);
    s.assert_nothing_submitted();
}

fn registered_without_rule() {
    // Registered with 10_000 spendable (no heartbeat shortfall), then
    // registered and unable to pay one heartbeat: the same setup line either
    // way, and never a funding park.
    for amount in [110_000, 100_014] {
        let s = Scene::new(
            World::new(amount, RegistryBox::Present),
            Endpoint::PortInline,
            None,
        );
        set_rule(missing_rule());
        let started = s.block_on(s.integ.start());
        assert!(
            started.is_ok() && !Scene::spawn_attempted(&started),
            "{amount}: {started:?}"
        );
        let expected = match amount {
            100_014 => format!("{SETUP} · This node's wallet also cannot pay the 0.001 ALGO fee of its next heartbeat — send 0.000986 ALGO to {ADDR}."),
            _ => SETUP.to_string(),
        };
        for check in 1..=3 {
            assert_setup_state(&s, &expected, &format!("{amount}, check {check}"));
        }
        assert!(
            s.log_lines("registration deferred").is_empty(),
            "{amount}: a registered node is never parked for funding"
        );
        s.assert_nothing_submitted();
    }
}

fn short_without_rule() {
    let s = Scene::new(
        World::new(110_000, RegistryBox::Absent),
        Endpoint::PortInline,
        None,
    );
    set_rule(missing_rule());
    let started = s.block_on(s.integ.start());
    assert!(started.is_ok() && !Scene::spawn_attempted(&started));
    let line = format!("{SETUP} · {}", registration_message(110_000));
    assert!(!line.contains('\n'), "one line");
    assert_setup_state(&s, &line, "check 1");

    // The shortfall figure follows the wallet on the ordinary backoff while
    // the rule is still missing; it never freezes.
    s.set_world(|w| w.amount = 150_000);
    let moved = format!("{SETUP} · {}", registration_message(150_000));
    let seen_at = (2..=6)
        .find(|_| s.block_on(s.integ.health_check()) == HealthStatus::Unhealthy(moved.clone()));
    assert_eq!(
        seen_at,
        Some(3),
        "re-read at the next due check (backoff 1, 2, …)"
    );

    // Funded: the shortfall leaves the line; the rule is still missing.
    s.set_world(|w| w.amount = 400_000);
    let seen_at = (4..=12)
        .find(|_| s.block_on(s.integ.health_check()) == HealthStatus::Unhealthy(SETUP.into()));
    assert!(seen_at.is_some(), "the shortfall part clears once funded");
    assert_eq!(s.parked(), None);
    s.assert_nothing_submitted();
}

fn rule_appears() {
    let s = Scene::new(
        World::new(400_000, RegistryBox::Present),
        Endpoint::PortInline,
        None,
    );
    set_rule(missing_rule());
    let started = s.block_on(s.integ.start());
    assert!(started.is_ok() && !Scene::spawn_attempted(&started));
    for check in 1..=3 {
        assert_setup_state(&s, SETUP, &format!("check {check}"));
    }

    // One Retry later the rule exists. The very next check sees it.
    set_rule(valid_rule());
    require_frynode_port_free();
    let status = s.block_on(s.integ.health_check());
    assert!(
        matches!(&status, HealthStatus::Unhealthy(r) if r.contains("frynode process is not running")),
        "the setup state lifts on the tick that finds the rule: {status:?}"
    );
    assert_eq!(
        recovery_action(&status, true, 0, 6),
        RecoveryAction::Restart,
        "and the supervisor's ordinary restart starts frynode"
    );
    let restarted = s.block_on(s.integ.start());
    assert!(Scene::spawn_attempted(&restarted), "{restarted:?}");
    s.assert_nothing_submitted();
}

#[cfg(not(windows))]
fn toggle_no_elevation() {
    let s = Scene::new(
        World::new(400_000, RegistryBox::Present),
        Endpoint::PortInline,
        None,
    );
    let absolute = s.tmp.path().join("resources").join("frynode");
    std::env::set_var("FRYNODE_BIN", &absolute);
    set_rule(missing_rule());

    let hardening_requests = || {
        s.log_lines("elevation suppressed")
            .into_iter()
            .filter(|l| l.contains("hardening"))
            .count()
    };

    let started = s.block_on(s.integ.start_for_user());
    assert!(
        s.log_lines("elevation allowed").is_empty(),
        "enabling fryDVPN must not raise a UAC prompt: {:?}",
        s.log_lines("elevation")
    );
    assert!(
        started.is_ok() && !Scene::spawn_attempted(&started),
        "{started:?}"
    );
    let blocks = crate::elevation_gate::blocked_reasons();
    assert_eq!(
        blocks.get("fryvpn"),
        None,
        "the start path makes no elevation request of its own: {blocks:?}"
    );
    assert_eq!(
        blocks.get("hardening").map(String::as_str),
        Some(crate::elevation_gate::NEEDS_APPROVAL_MESSAGE),
        "the setup state publishes the Security hardening banner the text points at: {blocks:?}"
    );
    assert_eq!(
        hardening_requests(),
        1,
        "requested once, refused by the gate"
    );

    // A second start while still waiting does not ask again.
    let again = s.block_on(s.integ.start_for_user());
    assert!(again.is_ok() && !Scene::spawn_attempted(&again));
    assert_eq!(
        hardening_requests(),
        1,
        "once per entry into the setup state"
    );

    // A stop leaves the setup state; the next start enters it again.
    s.block_on(s.integ.stop()).expect("stop");
    let after_stop = s.block_on(s.integ.start_for_user());
    assert!(after_stop.is_ok() && !Scene::spawn_attempted(&after_stop));
    assert_eq!(hardening_requests(), 2, "a new entry asks again");
    assert!(s.log_lines("elevation allowed").is_empty());
}

#[cfg(unix)]
fn running_rule_gone() {
    let s = Scene::new(
        World::new(400_000, RegistryBox::Present),
        Endpoint::PortInline,
        None,
    );
    install_decoy_frynode(&s);
    // Waiting on the rule first, then the rule appears: the setup state must
    // be gone for good once frynode is running.
    set_rule(missing_rule());
    let parked = s.block_on(s.integ.start());
    assert!(parked.is_ok() && !Scene::spawn_attempted(&parked));
    assert_setup_state(&s, SETUP, "before the rule");
    set_rule(valid_rule());
    require_frynode_port_free();
    let lifted = s.block_on(s.integ.health_check());
    assert_eq!(
        recovery_action(&lifted, true, 0, 6),
        RecoveryAction::Restart,
        "{lifted:?}"
    );
    let _health = frynode_health_decoy();
    let started = s.block_on(s.integ.start());
    assert!(started.is_ok(), "the decoy frynode must spawn: {started:?}");
    assert_eq!(s.block_on(s.integ.health_check()), HealthStatus::Healthy);

    // The rule disappears while frynode runs.
    set_rule(missing_rule());
    assert_eq!(
        s.block_on(s.integ.health_check()),
        HealthStatus::Healthy,
        "the rule gates spawning only; a running node keeps running"
    );
    let again = s.block_on(s.integ.start());
    assert!(again.is_ok(), "{again:?}");
    assert!(
        matches!(
            s.integ.supervisor.lock().unwrap().get_status("fryvpn"),
            HealthStatus::Healthy
        ),
        "a start on the running node leaves it running"
    );
    assert_eq!(s.block_on(s.integ.health_check()), HealthStatus::Healthy);
}

fn rule_gone_during_wallet_read() {
    let s = Scene::new(
        World {
            account_delay: Duration::from_secs(8),
            ..World::new(400_000, RegistryBox::Present)
        },
        Endpoint::PortInline,
        None,
    );
    set_rule(valid_rule());
    // The rule is deleted while start() waits on the wallet read.
    let deleter = std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(1));
        set_rule(missing_rule());
    });
    let started = s.block_on(s.integ.start());
    deleter.join().expect("deleter");
    assert!(
        started.is_ok() && !Scene::spawn_attempted(&started),
        "D-C5-2: a rule gone by the time of the spawn must stop it: {started:?}"
    );
    assert_setup_state(&s, SETUP, "rule gone during the wallet read");
    s.assert_nothing_submitted();
}

fn keeps_funding_park() {
    let s = Scene::new(
        World::new(110_000, RegistryBox::Absent),
        Endpoint::PortInline,
        None,
    );
    set_rule(valid_rule());
    let started = s.block_on(s.integ.start());
    let notice = registration_message(110_000);
    assert_eq!(s.parked(), Some(notice.clone()), "{started:?}");

    // The rule disappears and a start meets the parked node.
    set_rule(missing_rule());
    let again = s.block_on(s.integ.start());
    assert!(
        again.is_ok() && !Scene::spawn_attempted(&again),
        "{again:?}"
    );
    assert_eq!(
        s.parked(),
        Some(notice.clone()),
        "entering the setup state must not wipe the funding park and its notice"
    );
    assert_eq!(
        s.block_on(s.integ.health_check()),
        HealthStatus::Unhealthy(format!("{SETUP} · {notice}")),
        "the setup text leads, the shortfall follows"
    );

    // The rule is back: the funding park is still there, notice unchanged.
    set_rule(valid_rule());
    assert_eq!(
        s.block_on(s.integ.health_check()),
        HealthStatus::Unhealthy(notice.clone())
    );
    assert_eq!(s.parked(), Some(notice));
    s.assert_nothing_submitted();
}
