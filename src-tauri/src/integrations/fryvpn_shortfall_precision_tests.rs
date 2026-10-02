//! c4 BUG LOOP 9 (B8): the funding card's "send X ALGO" figure was the
//! shortfall rounded to the NEAREST 0.001 ALGO — an owner who sent exactly the
//! figure shown could still be up to 499 µALGO short, and a shortfall under
//! 500 µALGO read "send 0.000 ALGO". The figure shown must always be enough,
//! and the card's other two figures must agree with it.

use super::*;

const MIN_BAL: u64 = 100_000;
const ADDR: &str = "ADDR";

/// The card for a wallet exactly `short` µALGO below the total it needs.
fn message_for_shortfall(short: u64) -> String {
    let total = REGISTRATION_MIN_MICROALGOS + MIN_BAL;
    registration_funding_message(total - short, MIN_BAL, REGISTRATION_MIN_MICROALGOS, ADDR)
        .expect_err("a wallet below the total cannot afford registration")
}

/// An ALGO figure parsed back to µALGO with integer arithmetic, so the check
/// itself cannot round.
fn microalgos(figure: &str, msg: &str) -> u64 {
    let (whole, frac) = figure.split_once('.').unwrap_or((figure, ""));
    assert!(
        frac.len() <= 6 && frac.bytes().all(|b| b.is_ascii_digit()),
        "not a µALGO-precision figure: {figure:?} in {msg}"
    );
    let whole: u64 = whole
        .parse()
        .unwrap_or_else(|_| panic!("not a number: {figure:?} in {msg}"));
    let frac: u64 = format!("{frac:0<6}").parse().unwrap();
    whole * 1_000_000 + frac
}

fn figure_between<'a>(msg: &'a str, before: &str, after: &str) -> &'a str {
    let (_, rest) = msg
        .split_once(before)
        .unwrap_or_else(|| panic!("no {before:?} in {msg}"));
    let (figure, _) = rest
        .split_once(after)
        .unwrap_or_else(|| panic!("no {after:?} after {before:?} in {msg}"));
    figure
}

/// The "send X ALGO" figure, read the way the marker text promises it.
fn displayed_send_microalgos(msg: &str) -> u64 {
    let rest = msg
        .strip_prefix(&format!("{FUNDING_MARKER} — send "))
        .unwrap_or_else(|| panic!("the marker text must stay parseable: {msg}"));
    let (figure, _) = rest
        .split_once(&format!(" ALGO to {ADDR}"))
        .unwrap_or_else(|| panic!("no \"send … ALGO to\" figure: {msg}"));
    microalgos(figure, msg)
}

#[test]
fn a_shortfall_of_half_a_milli_algo_is_never_rounded_down() {
    let msg = message_for_shortfall(211_500);
    let sent = displayed_send_microalgos(&msg);
    assert!(
        sent >= 211_500,
        "sending the figure shown ({sent} µALGO) must cover the 211500 µALGO shortfall: {msg}"
    );
}

#[test]
fn a_shortfall_under_half_a_milli_algo_never_reads_zero() {
    let msg = message_for_shortfall(400);
    let sent = displayed_send_microalgos(&msg);
    assert!(sent > 0, "a 400 µALGO shortfall must not read as 0: {msg}");
    assert!(
        sent >= 400,
        "sending the figure shown ({sent} µALGO) must cover the 400 µALGO shortfall: {msg}"
    );
}

#[test]
fn a_whole_milli_algo_shortfall_is_quoted_exactly() {
    let msg = message_for_shortfall(312_000);
    assert_eq!(displayed_send_microalgos(&msg), 312_000, "{msg}");
}

/// A wallet opted into an app with local state has a minimum balance in half
/// milli-ALGO (0.1 base + 0.1 opt-in + 0.0285 per uint + 0.05 per byte slice),
/// so the total is not a whole 0.001 ALGO: it must still never be understated.
#[test]
fn a_half_milli_algo_total_is_never_understated() {
    const APP_OPTED_IN_MIN_BAL: u64 = 100_000 + 100_000 + 28_500 + 50_000;
    let total = REGISTRATION_MIN_MICROALGOS + APP_OPTED_IN_MIN_BAL;
    let msg =
        registration_funding_message(0, APP_OPTED_IN_MIN_BAL, REGISTRATION_MIN_MICROALGOS, ADDR)
            .expect_err("an empty wallet cannot afford registration");
    let needs = microalgos(figure_between(&msg, "needs ", " ALGO total"), &msg);
    assert!(needs >= total, "total {total} µALGO understated: {msg}");
    assert!(displayed_send_microalgos(&msg) >= total, "{msg}");
}

/// Every shortfall an owner can be in: the figure shown is enough and at most
/// one displayed 0.001 ALGO step over; the total is exact; the holdings are
/// never overstated; and "needs" less "holds" is exactly "send" — never
/// "needs 0.312 … holds 0.312 … send 0.001".
#[test]
fn every_shortfall_up_to_the_total_is_covered_by_the_figure_shown() {
    let total = REGISTRATION_MIN_MICROALGOS + MIN_BAL;
    for short in 1..=total {
        let msg = message_for_shortfall(short);
        let sent = displayed_send_microalgos(&msg);
        let needs = microalgos(figure_between(&msg, "needs ", " ALGO total"), &msg);
        let holds = microalgos(figure_between(&msg, "holds ", " ALGO)"), &msg);
        assert!(
            sent >= short && sent < short + 1_000,
            "short {short} µALGO, shown {sent} µALGO: {msg}"
        );
        assert_eq!(needs, total, "{msg}");
        assert!(holds <= total - short, "holdings overstated: {msg}");
        assert_eq!(needs - holds, sent, "the three figures disagree: {msg}");
    }
}
