//! BUG LOOP 3 — the scrubber never panics on a multibyte separator.
//!
//! The strict mnemonic rule separates words with the regex crate's Unicode
//! `\s`, so a run's last separator can be U+00A0, U+3000 or any other
//! multibyte White_Space. `mnemonic_replacement` located it with `rfind` and
//! then sliced one byte past it, which lands inside a multibyte separator and
//! panics — in the emitting task, a partner's log pump or a bundle export.
//! Values here are synthetic.

use super::*;
use std::panic::catch_unwind;

/// The first 24 words of the public BIP39 list; the 25th word is supplied by
/// each fixture.
const WORDS_24: &str = "abandon ability able about above absent absorb abstract absurd abuse access accident account accuse achieve acid acoustic acquire across act action actor actress actual";
const VALUE: &str = "Hunter2Secret";

fn every_unicode_whitespace() -> Vec<char> {
    (0..=0x10FFFFu32)
        .filter_map(char::from_u32)
        .filter(|c| c.is_whitespace())
        .collect()
}

/// Scrub `line` on both paths; a panic is reported, never propagated.
fn scrub_both(line: &str) -> Vec<(&'static str, Result<String, ()>)> {
    let paths: [(&'static str, fn(&str) -> String); 2] = [
        ("scrub_line", scrub_line),
        ("scrub_partner_line", scrub_partner_line),
    ];
    paths
        .into_iter()
        .map(|(path, scrub)| (path, catch_unwind(|| scrub(line)).map_err(|_| ())))
        .collect()
}

#[test]
fn nbsp_or_ideographic_space_before_an_assigned_name_never_panics() {
    for sep in ['\u{A0}', '\u{3000}'] {
        let line = format!("{WORDS_24}{sep}password: {VALUE}");
        for (path, out) in scrub_both(&line) {
            assert!(
                out.is_ok(),
                "{path} panicked on U+{:04X} before the run's last word",
                sep as u32
            );
            let out = out.unwrap();
            assert!(out.contains("[MNEMONIC]"), "{path}: {out:?}");
            assert!(!out.contains(VALUE), "{path}: the value survived: {out:?}");
        }
    }
}

/// Every Unicode whitespace character, in every position a run can end on:
/// before the last word, between all words, before a secret name with a value
/// and before an ordinary word with a colon after it.
#[test]
fn no_unicode_whitespace_separator_can_panic_the_scrubber() {
    let chars = every_unicode_whitespace();
    assert!(
        chars.len() >= 20,
        "fixture: {} whitespace chars",
        chars.len()
    );
    for sep in chars {
        let spaced = WORDS_24.replace(' ', &sep.to_string());
        for line in [
            format!("{WORDS_24}{sep}password: {VALUE}"),
            format!("{WORDS_24}{sep}token={VALUE}"),
            format!("{WORDS_24}{sep}adapt: noted"),
            format!("{spaced}{sep}secret= {VALUE}"),
            format!("loose {WORDS_24}{sep}adapt{sep}trailing"),
        ] {
            for (path, out) in scrub_both(&line) {
                assert!(
                    out.is_ok(),
                    "{path} panicked with separator U+{:04X}: {line:?}",
                    sep as u32
                );
            }
        }
    }
}
