//! P0b (rc39b): every standalone funding text is at most 60 words and keeps
//! every required element; the amounts equal the computations.
use super::*;
use crate::integrations::fryvpn::{
    heartbeat_shortfall_message, registration_affordability, registration_funding_message,
    rule_park_heartbeat_suffix, FUNDING_MARKER, REGISTRATION_MIN_MICROALGOS,
};
use crate::integrations::reward_model::SECOND_REQUIRED_BOOST;

const ADDR: &str = "SYNTHNODEWALLETADDRESS";
const MIN_BAL: u64 = 100_000;
const MAX_WORDS: usize = 60;

fn words(m: &str) -> usize {
    m.split_whitespace().count()
}

fn forbidden(m: &str) -> bool {
    let l = m.to_lowercase();
    l.contains("activat")
        || l.contains("opt in")
        || l.contains("opt-in")
        || l.split(|c: char| !c.is_alphanumeric()).any(|t| t == "asa")
}

fn boost() -> String {
    format!("{}%", (SECOND_REQUIRED_BOOST * 100.0).round() as u32)
}

fn algo3(micro: u64) -> String {
    format!("{:.3}", micro as f64 / 1_000_000.0)
}

fn fry_texts() -> Vec<(&'static str, String)> {
    vec![
        (
            "affordability",
            registration_affordability(0, 212_000, ADDR).unwrap_err(),
        ),
        (
            "funding",
            registration_funding_message(0, MIN_BAL, REGISTRATION_MIN_MICROALGOS, ADDR)
                .unwrap_err(),
        ),
        (
            "heartbeat",
            heartbeat_shortfall_message(100_000, MIN_BAL, ADDR).unwrap(),
        ),
        (
            "rule_park",
            rule_park_heartbeat_suffix(100_000, MIN_BAL, ADDR).unwrap(),
        ),
    ]
}

#[test]
fn every_fry_dvpn_funding_text_is_at_most_60_words_with_every_element() {
    for (name, m) in fry_texts() {
        assert!(words(&m) <= MAX_WORDS, "{name}: {} words: {m}", words(&m));
        assert!(!forbidden(&m), "{name}: forbidden word: {m}");
        assert!(m.contains("Fry dVPN"), "{name}: {m}");
        assert!(
            m.contains("device's own Fry dVPN node wallet"),
            "{name}: {m}"
        );
        assert!(m.contains("not a payment to anyone"), "{name}: {m}");
        assert!(m.contains("Optional"), "{name}: {m}");
        assert!(m.contains("switch Fry dVPN off"), "{name}: {m}");
        assert!(
            m.contains("Olostep alone earns the full base reward;"),
            "{name}: {m}"
        );
        assert!(
            m.ends_with(&format!(" adds {}.", boost()))
                || m.ends_with(&format!(" add {}.", boost())),
            "{name}: boost from the constant: {m}"
        );
        assert!(m.contains(ADDR), "{name}: {m}");
    }
}

#[test]
fn registration_texts_say_why_it_is_needed() {
    for (name, m) in fry_texts().into_iter().take(2) {
        assert!(m.contains("registration fees"), "{name}: {m}");
        assert!(m.contains("minimum balance"), "{name}: {m}");
    }
    let card =
        registration_funding_message(0, MIN_BAL, REGISTRATION_MIN_MICROALGOS, ADDR).unwrap_err();
    assert!(
        card.contains("not Fry's wallet or your reward wallet"),
        "{card}"
    );
}

#[test]
fn the_funding_card_starts_with_the_send_figure_and_equals_the_computation() {
    let total = REGISTRATION_MIN_MICROALGOS + MIN_BAL;
    let m =
        registration_funding_message(0, MIN_BAL, REGISTRATION_MIN_MICROALGOS, ADDR).unwrap_err();
    let head = format!("{FUNDING_MARKER} — send {} ALGO to {ADDR}", algo3(total));
    assert!(m.starts_with(&head), "{m}");
    assert!(
        m.contains(&format!("needs {} ALGO total", algo3(total))),
        "{m}"
    );
    assert!(m.contains("holds 0.000 ALGO)"), "{m}");
    assert!(crate::integrations::awaits_user_action(&m), "{m}");
}

#[test]
fn the_sentinel_unfunded_text_is_at_most_60_words_with_every_element() {
    let m = SentinelIntegration::unfunded_message("sent1qqqqexample");
    assert!(words(&m) <= MAX_WORDS, "{} words: {m}", words(&m));
    assert!(!forbidden(&m), "{m}");
    assert!(m.contains("Sentinel dVPN"), "{m}");
    assert!(m.contains("this device's own Sentinel node account"), "{m}");
    assert!(m.contains("register and pay Sentinel network fees"), "{m}");
    assert!(m.contains("not a Fry token"), "{m}");
    assert!(m.contains("Sentinel network's token"), "{m}");
    assert!(m.contains("Not a payment to Fry or anyone"), "{m}");
    assert!(m.contains("Optional"), "{m}");
    assert!(m.contains("turn Sentinel dVPN off with its switch"), "{m}");
    assert!(m.contains("sent1qqqqexample"), "{m}");
    assert!(crate::integrations::awaits_user_action(&m), "{m}");
}

/// F1: the "Send" figure rounds UP to a whole 0.001 ALGO, so sending it is
/// always enough (a nearest-rounded 0.2114 would read 0.211 and fall short).
#[test]
fn the_affordability_send_figure_rounds_up_and_never_understates() {
    let m = registration_affordability(100_600, 312_000, ADDR).unwrap_err();
    assert!(m.contains(&format!("Send 0.212 ALGO to {ADDR}")), "{m}");
    let m = registration_affordability(0, 211_400, ADDR).unwrap_err();
    assert!(m.contains(&format!("Send 0.212 ALGO to {ADDR}")), "{m}");
    assert!(words(&m) <= MAX_WORDS, "{m}");
}
