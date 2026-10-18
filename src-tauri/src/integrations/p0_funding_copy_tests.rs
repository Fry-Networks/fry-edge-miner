//! P0 (rc39): the fryDVPN funding texts name the integration, say what the
//! wallet is for, say it is optional and not a payment to Fry, and quote the
//! amounts the pre-check computes. (a) wording, (b) amounts.
use super::*;
use crate::integrations::reward_model::SECOND_REQUIRED_BOOST;

const ADDR: &str = "SYNTHNODEWALLETADDRESS";
const MIN_BAL: u64 = 100_000;

fn forbidden_words(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut bad: Vec<String> = ["activat", "opt in", "opt-in", "optin", "opt_in"]
        .iter()
        .filter(|w| lower.contains(*w))
        .map(|w| w.to_string())
        .collect();
    if lower
        .split(|c: char| !c.is_alphanumeric())
        .any(|t| t == "asa")
    {
        bad.push("asa".to_string());
    }
    bad
}

fn boost_text() -> String {
    format!("{}%", (SECOND_REQUIRED_BOOST * 100.0).round() as u32)
}

fn algo3(micro: u64) -> String {
    format!("{:.3}", micro as f64 / 1_000_000.0)
}

fn standalone_texts() -> Vec<(&'static str, String)> {
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
    ]
}

fn assert_plain_copy(name: &str, m: &str) {
    assert!(
        forbidden_words(m).is_empty(),
        "{name}: {:?}: {m}",
        forbidden_words(m)
    );
    assert!(m.contains("Fry dVPN"), "{name}: names the integration: {m}");
    assert!(
        m.contains("node wallet"),
        "{name}: says what the wallet is: {m}"
    );
    assert!(
        m.contains("own Fry dVPN node wallet"),
        "{name}: this device's own: {m}"
    );
    assert!(
        m.contains("not a payment to anyone"),
        "{name}: not a payment: {m}"
    );
    assert!(
        m.contains("not Fry's wallet"),
        "{name}: not Fry's wallet: {m}"
    );
    assert!(m.split_whitespace().count() <= 95, "{name}: too long: {m}");
    assert!(
        m.to_lowercase().contains("optional"),
        "{name}: optional: {m}"
    );
    assert!(
        m.contains("Olostep alone earns the full base reward"),
        "{name}: {m}"
    );
    assert!(
        m.contains(&boost_text()),
        "{name}: boost from the constant: {m}"
    );
    assert!(m.contains("switch"), "{name}: points at the switch: {m}");
    assert!(
        m.contains("Optional: you can earn without it by running Olostep instead."),
        "{name}: earning path: {m}"
    );
    assert!(
        m.contains("turn Fry dVPN off with its switch and turn Olostep on."),
        "{name}: skip path: {m}"
    );
}

#[test]
fn every_standalone_fry_dvpn_funding_text_is_plain_and_optional() {
    for (name, m) in standalone_texts() {
        assert_plain_copy(name, &m);
        assert!(m.contains(ADDR), "{name}: keeps the address: {m}");
    }
}

#[test]
fn the_funding_card_keeps_its_marker_and_stays_short() {
    let m =
        registration_funding_message(0, MIN_BAL, REGISTRATION_MIN_MICROALGOS, ADDR).unwrap_err();
    assert!(m.starts_with(FUNDING_MARKER), "{m}");
    assert!(crate::integrations::awaits_user_action(&m));
    assert!(m.split_whitespace().count() <= 90, "too long: {m}");
}

#[test]
fn the_rule_park_suffix_is_plain_and_optional() {
    let m = rule_park_heartbeat_suffix(100_000, MIN_BAL, ADDR).unwrap();
    assert!(forbidden_words(&m).is_empty(), "{m}");
    assert!(m.contains("Fry dVPN"), "{m}");
    assert!(m.contains("own Fry dVPN node wallet"), "{m}");
    assert!(m.contains("not a payment to anyone"), "{m}");
    assert!(m.to_lowercase().contains("optional"), "{m}");
    assert!(
        m.contains("you can earn without it by running Olostep instead"),
        "{m}"
    );
    assert!(
        m.contains("turn Fry dVPN off with its switch and turn Olostep on."),
        "{m}"
    );
    assert!(m.contains("also cannot pay"), "{m}");
    assert!(m.split_whitespace().count() <= 65, "too long: {m}");
}

/// (b) the funding card quotes exactly what the pre-check computes.
#[test]
fn funding_card_amounts_equal_the_precheck_for_three_balances() {
    let total = REGISTRATION_MIN_MICROALGOS + MIN_BAL;
    for amount in [0u64, 100_000, total - 1] {
        let m = registration_funding_message(amount, MIN_BAL, REGISTRATION_MIN_MICROALGOS, ADDR)
            .unwrap_err();
        let short = total - amount;
        let send = format!(
            "To use Fry dVPN, send {} ALGO to {ADDR}",
            algo3(short.div_ceil(1_000) * 1_000)
        );
        let needs = format!("needs {} ALGO total", algo3(total.div_ceil(1_000) * 1_000));
        let holds = format!("holds {} ALGO)", algo3(amount / 1_000 * 1_000));
        for want in [send, needs, holds] {
            assert!(
                m.contains(&want),
                "amount {amount}: missing {want:?} in {m}"
            );
        }
    }
}

#[test]
fn affordability_amounts_equal_the_precheck_for_three_balances() {
    let required = 212_000u64;
    for spendable in [0u64, 100_000, required - 1] {
        let m = registration_affordability(spendable, required, ADDR).unwrap_err();
        assert!(
            m.contains(&format!("To use Fry dVPN, send ALGO to {ADDR}")),
            "{m}"
        );
        for want in [
            algo3(required),
            algo3(spendable),
            algo3(required - spendable),
        ] {
            assert!(
                m.contains(&format!("{want} ALGO")),
                "spendable {spendable}: {want} in {m}"
            );
        }
    }
}

#[test]
fn heartbeat_amounts_equal_the_precheck_for_three_balances() {
    for amount in [0u64, 100_000, 100_000 + HEARTBEAT_FEE_MICROALGOS - 1] {
        let short = MIN_BAL + HEARTBEAT_FEE_MICROALGOS - amount;
        let figure = format!("send {:.6} ALGO to {ADDR}", short as f64 / 1_000_000.0);
        let want_fee = format!(
            "{} ALGO fee of its next heartbeat",
            algo3(HEARTBEAT_FEE_MICROALGOS)
        );
        let n = heartbeat_shortfall_message(amount, MIN_BAL, ADDR).unwrap();
        let s = rule_park_heartbeat_suffix(amount, MIN_BAL, ADDR).unwrap();
        for (m, lead) in [
            (n, "To keep Fry dVPN running, "),
            (s, "To keep Fry dVPN running — "),
        ] {
            let want_send = format!("{lead}{figure}");
            assert!(
                m.contains(&want_send),
                "amount {amount}: {want_send} in {m}"
            );
            assert!(m.contains(&want_fee), "amount {amount}: {want_fee} in {m}");
        }
    }
}
