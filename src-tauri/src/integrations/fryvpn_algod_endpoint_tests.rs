//! FAIL-12 — FEM's own algod reads must honour the overrides frynode already
//! gets.
//!
//! `start_inner` hands frynode `-algod-server`, `-algod-port` and
//! `-algod-token`, but FEM's balance read built its URL from the server alone
//! and sent no token. Against a token-protected algod — AlgoKit LocalNet, the
//! only place the registration path can be exercised — that read the wrong
//! port or got a 401, and the funding gate saw an unreadable wallet instead of
//! a balance.
//!
//! Each scenario runs in a child process; see `fryvpn_c4_support`.

use super::fryvpn_c4_support::*;
use super::*;

#[test]
fn the_balance_read_sends_the_configured_algod_token() {
    run_scenario(module_path!(), "token_required");
}

#[test]
fn the_balance_read_adds_the_configured_port_to_a_server_without_one() {
    run_scenario(module_path!(), "port_override");
}

#[test]
fn a_port_already_in_the_server_string_is_kept() {
    run_scenario(module_path!(), "port_inline_wins");
}

#[test]
fn no_token_header_is_sent_when_no_token_is_configured() {
    run_scenario(module_path!(), "no_token");
}

#[test]
fn the_funding_gate_measures_the_wallet_through_the_overrides() {
    run_scenario(module_path!(), "gate_through_overrides");
}

#[test]
#[ignore = "scenario entry point: the tests above run it in a child process"]
fn child() {
    let Ok(scenario) = std::env::var(SCENARIO_VAR) else {
        return;
    };
    match scenario.as_str() {
        "token_required" => token_required(),
        "port_override" => port_override(),
        "port_inline_wins" => port_inline_wins(),
        "no_token" => no_token(),
        "gate_through_overrides" => gate_through_overrides(),
        other => panic!("unknown scenario {other}"),
    }
    println!("{DONE} {scenario}");
}

fn token_required() {
    let s = Scene::new(
        World {
            require_token: true,
            ..World::new(1_234_567, RegistryBox::Absent)
        },
        Endpoint::PortInline,
        Some(TOKEN),
    );
    let read = s.block_on(FryVpnIntegration::read_wallet_balance(ADDR));
    assert_eq!(
        read,
        Ok((1_234_567, MIN_BALANCE)),
        "FAIL-12: with FRYNODE_ALGOD_TOKEN set, FEM's balance read must authenticate the way \
         frynode does; a token-protected algod (LocalNet) answers 401 and the wallet reads as \
         unmeasurable"
    );
    let sent = s.algod.requests_to(&account_path());
    assert_eq!(sent.len(), 1, "exactly one account read: {sent:?}");
    assert_eq!(
        sent[0].header("X-Algo-API-Token"),
        Some(TOKEN),
        "the token travels in algod's own header"
    );
}

fn port_override() {
    let s = Scene::new(
        World {
            require_token: true,
            ..World::new(1_234_567, RegistryBox::Absent)
        },
        Endpoint::PortOverride,
        Some(TOKEN),
    );
    let read = s.block_on(FryVpnIntegration::read_wallet_balance(ADDR));
    assert_eq!(
        read,
        Ok((1_234_567, MIN_BALANCE)),
        "FAIL-12: a bare FRYNODE_ALGOD_SERVER plus FRYNODE_ALGOD_PORT is exactly how frynode is \
         configured (it joins them as server:port); FEM must read the same endpoint, not the \
         scheme's default port"
    );
    assert_eq!(s.reads(&account_path()), 1);
}

fn port_inline_wins() {
    let s = Scene::new(
        World::new(1_234_567, RegistryBox::Absent),
        Endpoint::PortInline,
        Some(TOKEN),
    );
    // A port override that disagrees with the server string's own port.
    std::env::set_var("FRYNODE_ALGOD_PORT", "1");
    let read = s.block_on(FryVpnIntegration::read_wallet_balance(ADDR));
    assert_eq!(
        read,
        Ok((1_234_567, MIN_BALANCE)),
        "a server string that already names its port is used as is — appending \
         FRYNODE_ALGOD_PORT to it again builds a URL nothing answers"
    );
    assert_eq!(s.reads(&account_path()), 1);
}

fn no_token() {
    for token in [None, Some(""), Some("   ")] {
        let s = Scene::new(
            World::new(1_234_567, RegistryBox::Absent),
            Endpoint::PortInline,
            token,
        );
        let read = s.block_on(FryVpnIntegration::read_wallet_balance(ADDR));
        assert_eq!(read, Ok((1_234_567, MIN_BALANCE)), "token {token:?}");
        let sent = s.algod.requests_to(&account_path());
        assert_eq!(sent.len(), 1, "token {token:?}: {sent:?}");
        assert_eq!(
            sent[0].header("X-Algo-API-Token"),
            None,
            "the shipped endpoint (algonode) is tokenless: an unset or blank token {token:?} \
             must not be sent as a header"
        );
    }
}

/// The same overrides, through the real start path: the gate must MEASURE the
/// wallet (and park an underfunded one with its exact figure) rather than
/// treat a 401 as an unmeasurable wallet and start frynode anyway.
fn gate_through_overrides() {
    let s = Scene::new(
        World {
            require_token: true,
            ..World::new(110_000, RegistryBox::Absent)
        },
        Endpoint::PortOverride,
        Some(TOKEN),
    );
    let started = s.block_on(s.integ.start());
    let expected =
        registration_funding_message(110_000, MIN_BALANCE, REGISTRATION_MIN_MICROALGOS, ADDR)
            .expect_err("fixture: 110_000 cannot afford registration");
    assert_eq!(
        s.parked(),
        Some(expected),
        "FAIL-12: the funding gate must read the wallet through the configured algod and park \
         with the measured shortfall (start returned {started:?})"
    );
    assert!(
        started.is_ok(),
        "a parked start succeeds without spawning: {started:?}"
    );
    assert!(
        s.reads(&account_path()) >= 1,
        "the gate never reached the decoy algod"
    );
    s.assert_nothing_submitted();
}
