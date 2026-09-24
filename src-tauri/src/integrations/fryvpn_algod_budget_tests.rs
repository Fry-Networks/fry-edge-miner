//! FAIL-2 follow-up — the registry-box read must not lengthen the toggle.
//!
//! `start_for_user` runs under TOGGLE_STEP_TIMEOUT (60 s). Its worst case
//! before FAIL-2 was already the firewall probe (20 s) + the credentials
//! lookup (30 s) + the balance read (10 s) = 60 s. The box read FAIL-2 added
//! had a 5 s budget of its own, so a slow algod pushed the toggle to 65 s and
//! the card flipped back off — the B7/B8 shape. Both reads now share the
//! balance read's 10 s.
//!
//! Each scenario runs in a child process; see `fryvpn_c4_support`.

use super::fryvpn_c4_support::*;
use super::*;
use std::time::Instant;

/// The 10 s budget plus loopback and scheduling slack. The credentials lookup
/// answers at once, so all of this is algod time; a box read with a budget of
/// its own takes both scenarios to about 12 s.
const BOUND: Duration = Duration::from_secs(11);

#[test]
fn a_slow_balance_read_leaves_the_box_read_only_the_rest_of_the_budget() {
    run_scenario(module_path!(), "slow_balance_then_slow_box");
}

#[test]
fn a_balance_read_that_spends_the_budget_leaves_no_box_read() {
    run_scenario(module_path!(), "balance_times_out");
}

#[test]
#[ignore = "scenario entry point: the tests above run it in a child process"]
fn child() {
    let Ok(scenario) = std::env::var(SCENARIO_VAR) else {
        return;
    };
    match scenario.as_str() {
        "slow_balance_then_slow_box" => slow_balance_then_slow_box(),
        "balance_times_out" => balance_times_out(),
        other => panic!("unknown scenario {other}"),
    }
    println!("{DONE} {scenario}");
}

/// 7 s for the balance, then a box read that would take 7 s more. With a
/// budget of its own the box read ran to its 5 s timeout: 12 s of algod.
fn slow_balance_then_slow_box() {
    let s = Scene::new(
        World {
            account_delay: Duration::from_secs(7),
            box_delay: Duration::from_secs(7),
            ..World::new(110_000, RegistryBox::Present)
        },
        Endpoint::PortInline,
        None,
    );
    let began = Instant::now();
    let started = s.block_on(s.integ.start());
    let took = began.elapsed();
    assert!(
        took >= Duration::from_secs(7),
        "the balance read must really have waited: {took:?}"
    );
    assert!(
        took <= BOUND,
        "the balance and box reads together must stay inside the balance read's 10 s, so \
         the toggle keeps its 60 s bound: took {took:?}"
    );
    assert_eq!(
        s.parked(),
        Some(
            registration_funding_message(110_000, MIN_BALANCE, REGISTRATION_MIN_MICROALGOS, ADDR)
                .expect_err("fixture: 110_000 cannot afford registration")
        ),
        "a box read cut short proves nothing, so the registration gate stands (start \
         returned {started:?})"
    );
    s.assert_nothing_submitted();
}

/// algod never answers the balance inside the budget: nothing is left for
/// the box read, registration is unknown, and the node stays up.
fn balance_times_out() {
    let s = Scene::new(
        World {
            account_delay: Duration::from_millis(11_500),
            ..World::new(110_000, RegistryBox::Present)
        },
        Endpoint::PortInline,
        None,
    );
    let began = Instant::now();
    let started = s.block_on(s.integ.start());
    let took = began.elapsed();
    assert!(
        took <= BOUND,
        "a balance read that spent the whole budget must not be followed by another \
         algod wait: took {took:?}"
    );
    assert_eq!(
        s.parked(),
        None,
        "availability: unreadable balance and unknown registration keep the node up"
    );
    assert!(Scene::spawn_attempted(&started), "{started:?}");
    assert_eq!(
        s.log_lines("no time left in the algod read budget").len(),
        1,
        "with the budget spent, registration is Unknown without another request"
    );
    s.assert_nothing_submitted();
}
