//! P0: the Sentinel unfunded-account card text.
use super::*;

fn msg() -> String {
    SentinelIntegration::unfunded_message("sent1qqqqexample")
}

#[test]
fn unfunded_message_keeps_the_awaiting_marker_and_the_address() {
    let m = msg();
    assert!(m.contains("node account not funded"), "{m}");
    assert!(m.contains("sent1qqqqexample"), "{m}");
    assert!(crate::integrations::awaits_user_action(&m));
}

#[test]
fn unfunded_message_is_plain_optional_and_not_a_payment_to_fry() {
    let m = msg();
    let lower = m.to_lowercase();
    for w in ["activat", "opt in", "opt-in", "optin"] {
        assert!(!lower.contains(w), "{w}: {m}");
    }
    assert!(!lower.split(|c: char| !c.is_alphanumeric()).any(|t| t == "asa"), "{m}");
    assert!(m.contains("Sentinel dVPN"), "{m}");
    assert!(m.contains("Sentinel network's token"), "{m}");
    assert!(m.contains("not a Fry token"), "{m}");
    assert!(m.contains("not a payment to Fry or anyone"), "{m}");
    assert!(lower.contains("optional"), "{m}");
    assert!(m.contains("does not affect your other rewards"), "{m}");
    assert!(m.contains("switch"), "{m}");
    assert!(m.split_whitespace().count() <= 65, "too long: {m}");
}

#[test]
fn unfunded_message_quotes_the_minimum_from_the_constant() {
    let m = msg();
    let min = SentinelIntegration::MIN_FUNDING_UDVPN / 1_000_000;
    assert!(m.contains(&format!("at least {min} DVPN")), "{m}");
}
