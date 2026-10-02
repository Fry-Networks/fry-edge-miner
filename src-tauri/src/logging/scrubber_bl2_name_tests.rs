//! BUG LOOP 2 — a secret NAME right after a phrase keeps its value redacted.
//!
//! Row 4 made both mnemonic rules open-ended so a prose-prefixed phrase is
//! taken whole. They run before every name-keyed rule, and their last word
//! also accepts a word followed by `=` or `:`, so a run could end by
//! swallowing a secret's NAME — `… adapt password=X` became `[MNEMONIC]=X` —
//! and no rule after it could see the value any more. The values here are
//! synthetic.

use super::*;

/// The first 25 words of the public BIP39 list.
const PHRASE: &str = "abandon ability able about above absent absorb abstract absurd abuse access accident account accuse achieve acid acoustic acquire across act action actor actress actual adapt";
const VALUE: &str = "Hunter2Secret";

fn words() -> Vec<&'static str> {
    PHRASE.split(' ').collect()
}

fn capitalised() -> String {
    words()
        .iter()
        .map(|w| format!("{}{}", w[..1].to_ascii_uppercase(), &w[1..]))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A phrase directly followed by a secret name and its value, in the shapes
/// the name-keyed rules redact on their own.
fn phrase_then_secret() -> Vec<String> {
    vec![
        format!("seed {PHRASE} token: {VALUE}"),
        format!("restore phrase {PHRASE} password={VALUE}"),
        format!("{PHRASE} secret={VALUE}"),
        format!("{PHRASE} usertoken = {VALUE}"),
        format!("Seed {} Password={VALUE}", capitalised()),
        format!("seed {}, password={VALUE}", words().join(", ")),
    ]
}

fn assert_nothing_survives(scrub: fn(&str) -> String, path: &str) {
    for line in phrase_then_secret() {
        let out = scrub(&line);
        assert!(
            !out.contains(VALUE),
            "{path}: the secret after the phrase survived: {out}"
        );
        assert!(out.contains("[MNEMONIC]"), "{path}: {out}");
        for word in words() {
            assert!(
                !out.to_ascii_lowercase()
                    .split(|c: char| !c.is_ascii_alphabetic())
                    .any(|w| w == word),
                "{path}: the phrase word {word:?} survived: {out}"
            );
        }
        assert_eq!(scrub(&out), out, "{path}: must stay idempotent");
    }
}

#[test]
fn a_secret_named_right_after_a_phrase_is_still_redacted() {
    assert_nothing_survives(scrub_line, "scrub_line");
}

#[test]
fn the_partner_scrubber_keeps_the_name_too() {
    assert_nothing_survives(scrub_partner_line, "scrub_partner_line");
}

/// Only a secret NAME is kept: an ordinary phrase word followed by `:` or `=`
/// is still a phrase word.
#[test]
fn a_phrase_word_before_a_colon_is_not_kept() {
    for line in [format!("{PHRASE}: noted"), format!("loose {PHRASE}=1")] {
        let out = scrub_line(&line);
        assert!(!out.contains("adapt"), "{out}");
    }
}

/// A phrase word that happens to be a secret name ("secret" and "token" are
/// both BIP39 words) is only kept when a value follows it. At the end of a
/// run, or before prose, it is a phrase word and goes with the rest.
#[test]
fn a_phrase_ending_in_a_name_like_word_is_still_redacted_whole() {
    let mut phrase = words();
    for last in ["secret", "token"] {
        phrase[24] = last;
        let phrase = phrase.join(" ");
        for line in [phrase.clone(), format!("loose {phrase} trailing")] {
            let out = scrub_line(&line);
            for word in phrase.split(' ') {
                assert!(
                    !out.split(|c: char| !c.is_ascii_alphabetic())
                        .any(|w| w == word),
                    "the phrase word {word:?} survived: {out}"
                );
            }
        }
    }
}
