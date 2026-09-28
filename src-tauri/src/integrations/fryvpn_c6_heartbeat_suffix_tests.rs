//! Continuation #6, D-C6-1: the heartbeat text a registered node waiting on
//! its firewall rule shows after the setup line — D-C4-2's figures, in
//! D-C4-2's format.

use super::*;

const LAB_ADDR: &str = "FZOSX4K5D3I5ILTF7KQ2K45JAX2HANZHCE23BKMV2HQQ4KSEMIRL5DVCQM";

#[test]
fn the_lab_example_reads_exactly_as_ruled() {
    let suffix = rule_park_heartbeat_suffix(100_999, 100_000, LAB_ADDR);
    assert_eq!(
        suffix.as_deref(),
        Some(
            "This node's wallet also cannot pay the 0.001 ALGO fee of its next heartbeat \
             — send 0.000001 ALGO to FZOSX4K5D3I5ILTF7KQ2K45JAX2HANZHCE23BKMV2HQQ4KSEMIRL5DVCQM."
        )
    );
    let line = firewall_setup_reason(suffix);
    assert_eq!(
        line,
        "Awaiting administrator action — Click Retry on the Security hardening banner to \
         allow fryDVPN through Windows Firewall. · This node's wallet also cannot pay the \
         0.001 ALGO fee of its next heartbeat — send 0.000001 ALGO to \
         FZOSX4K5D3I5ILTF7KQ2K45JAX2HANZHCE23BKMV2HQQ4KSEMIRL5DVCQM."
    );
    assert_eq!(line.chars().filter(|&c| c == '\u{2014}').count(), 2);
    assert_eq!(line.chars().filter(|&c| c == '\u{00B7}').count(), 1);
    assert_eq!(line.matches(" \u{00B7} ").count(), 1);
    assert!(!line.contains('\n'));
}

#[test]
fn the_fee_renders_from_the_heartbeat_constant() {
    let fee = format!("{:.3}", HEARTBEAT_FEE_MICROALGOS as f64 / 1_000_000.0);
    assert_eq!(fee, "0.001");
    let suffix = rule_park_heartbeat_suffix(0, 0, LAB_ADDR).expect("an empty wallet is short");
    assert!(
        suffix.contains(&format!("pay the {fee} ALGO fee")),
        "{suffix}"
    );
}

/// The "send X ALGO to ADDR." tail of either text.
fn send_tail(text: &str) -> &str {
    let at = text.find("send ").expect("a send instruction");
    let end = text[at..].find(". ").map_or(text.len(), |e| at + e + 1);
    &text[at..end]
}

#[test]
fn the_figures_match_the_dc42_notice() {
    for (amount, min_balance, short) in [
        (100_999, 100_000, Some("0.000001")),
        (100_014, 100_000, Some("0.000986")),
        (50_000, 100_000, Some("0.051000")),
        (0, 0, Some("0.001000")),
        (101_000, 100_000, None),
        (400_000, 100_000, None),
    ] {
        let dc42 = heartbeat_shortfall_message(amount, min_balance, LAB_ADDR);
        let suffix = rule_park_heartbeat_suffix(amount, min_balance, LAB_ADDR);
        assert_eq!(dc42.is_some(), suffix.is_some(), "{amount}/{min_balance}");
        if let (Some(dc42), Some(suffix)) = (dc42, suffix) {
            let expected = format!("send {} ALGO to {LAB_ADDR}.", short.unwrap());
            assert_eq!(send_tail(&dc42), expected, "{amount}/{min_balance}");
            assert_eq!(send_tail(&suffix), expected, "{amount}/{min_balance}");
        } else {
            assert_eq!(short, None, "{amount}/{min_balance}");
        }
    }
}

#[test]
fn a_wallet_that_can_pay_gets_no_suffix() {
    // Exactly one heartbeat spendable, and an Affordable wallet.
    assert_eq!(rule_park_heartbeat_suffix(101_000, 100_000, LAB_ADDR), None);
    assert_eq!(rule_park_heartbeat_suffix(400_000, 100_000, LAB_ADDR), None);
}
