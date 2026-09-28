//! Continuation #5, D-C5-5: the three figures on fryDVPN's funding card
//! (b252ac6: send and total rounded UP to 0.001 ALGO, holdings DOWN) and
//! whether shown total − shown holdings equals shown send. Either answer is
//! acceptable; this prints the case table (run with --nocapture) and pins what
//! must hold in every row: the send figure is never below the true shortfall,
//! never "0.000", and each figure is its rounding of the exact value.

use super::*;

/// Synthetic, checksum-valid Algorand address — not a real wallet.
const ADDR: &str = "YTC4NR6IZHFMXTGNZ3H5BUOS2PKNLVWX3DM5VW643XPN7YHB4LRUS2CHMA";

/// (account min-balance, amount held), in µALGO. A min-balance that is not a
/// whole 0.001 ALGO (128_500: one app opt-in with a local uint) is what puts a
/// remainder into the total.
const CASES: [(u64, u64); 14] = [
    (100_000, 311_999),
    (100_000, 311_501),
    (100_000, 311_500),
    (100_000, 311_499),
    (100_000, 311_001),
    (100_000, 311_000),
    (100_000, 100_000),
    (100_000, 0),
    (128_500, 200_499),
    (128_500, 200_500),
    (128_500, 200_999),
    (100_001, 200_000),
    (100_001, 200_001),
    (100_001, 312_000),
];

/// The figure after `before` in the card text, in thousandths of an ALGO.
fn figure(msg: &str, before: &str) -> u64 {
    let at = msg.find(before).expect("figure label") + before.len();
    let text: String = msg[at..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let (whole, frac) = text.split_once('.').expect("three decimals");
    assert_eq!(frac.len(), 3, "{text}");
    whole.parse::<u64>().unwrap() * 1_000 + frac.parse::<u64>().unwrap()
}

#[test]
fn the_funding_card_figures_table() {
    println!(
        "| # | min-balance | holds (µALGO) | total needed | true shortfall | shown send | shown total | shown holdings | total − holdings | == send? |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|");
    let mut unequal = 0;
    for (row, (min_balance, amount)) in CASES.into_iter().enumerate() {
        let msg =
            registration_funding_message(amount, min_balance, REGISTRATION_MIN_MICROALGOS, ADDR)
                .expect_err("every case is short of the registration price");
        let total_needed = REGISTRATION_MIN_MICROALGOS + min_balance;
        let short = total_needed - amount;
        let (send, total, holds) = (
            figure(&msg, "send "),
            figure(&msg, "needs "),
            figure(&msg, "holds "),
        );

        assert!(
            send * 1_000 >= short,
            "row {row}: asks for less than the shortfall: {msg}"
        );
        assert!(send > 0, "row {row}: never 0.000: {msg}");
        assert_eq!(send, short.div_ceil(1_000), "row {row}: send rounds UP");
        assert_eq!(
            total,
            total_needed.div_ceil(1_000),
            "row {row}: total rounds UP"
        );
        assert_eq!(holds, amount / 1_000, "row {row}: holdings round DOWN");

        let equal = total - holds == send;
        // Derivation: the two agree unless the total has a sub-0.001 remainder
        // and the holdings' remainder is at least as large.
        let predicted = total_needed.is_multiple_of(1_000) || amount % 1_000 < total_needed % 1_000;
        assert_eq!(equal, predicted, "row {row}");
        if !equal {
            unequal += 1;
            assert_eq!(total - holds, send + 1, "row {row}: off by exactly 0.001");
        }
        println!(
            "| {} | {min_balance} | {amount} | {total_needed} | {short} | {:.3} | {:.3} | {:.3} | {:.3} | {} |",
            row + 1,
            send as f64 / 1_000.0,
            total as f64 / 1_000.0,
            holds as f64 / 1_000.0,
            (total - holds) as f64 / 1_000.0,
            if equal { "yes" } else { "NO (+0.001)" }
        );
    }
    println!(
        "{unequal} of {} rows differ; every row with the standard 0.1 ALGO minimum agrees.",
        CASES.len()
    );
    assert!(
        unequal > 0 && unequal < CASES.len(),
        "the table covers both outcomes"
    );
}
