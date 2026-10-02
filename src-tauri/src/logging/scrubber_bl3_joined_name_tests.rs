//! BUG LOOP 3 — a JOINED secret name right after a phrase keeps its value
//! redacted.
//!
//! The name-keep fix recognised only single-word names. A run's last word
//! stops at '.' or '-', so for `… adapt api-key: X` the run swallowed `api`,
//! nothing assigned directly followed it, and `[MNEMONIC]-key: X` was left
//! with no name any later rule could see — on 0.4.33 this shape was
//! redacted. Values here are synthetic.

use super::*;

/// The first 25 words of the public BIP39 list.
const PHRASE: &str = "abandon ability able about above absent absorb abstract absurd abuse access accident account accuse achieve acid acoustic acquire across act action actor actress actual adapt";
const VALUE: &str = "Hunter2Secret";

/// The joined names the name-keyed rules redact on their own.
const JOINED: [&str; 8] = [
    "api-key: ",
    "node-token=",
    "user.token=",
    "private-key=",
    "seed-phrase=",
    "client.secret: ",
    "access-key = ",
    "auth-token: ",
];

fn phrase_words() -> Vec<&'static str> {
    PHRASE.split(' ').collect()
}

fn capitalised() -> String {
    phrase_words()
        .iter()
        .map(|w| format!("{}{}", w[..1].to_ascii_uppercase(), &w[1..]))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `name` is the joined name the line carries: its segments are the name, not
/// phrase words, even where one is spelled like a phrase word ("access").
fn assert_clean(scrub: fn(&str) -> String, path: &str, line: &str, name: &str) {
    let name = name.to_ascii_lowercase();
    let name_segments: Vec<&str> = name
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|w| !w.is_empty())
        .collect();
    let out = scrub(line);
    assert!(
        !out.contains(VALUE),
        "{path}: the value after a joined name survived: {out}"
    );
    assert!(out.contains("[MNEMONIC]"), "{path}: {out}");
    for word in phrase_words()
        .into_iter()
        .filter(|w| !name_segments.contains(w))
    {
        assert!(
            !out.to_ascii_lowercase()
                .split(|c: char| !c.is_ascii_alphabetic())
                .any(|w| w == word),
            "{path}: the phrase word {word:?} survived: {out}"
        );
    }
    assert_eq!(scrub(&out), out, "{path}: must stay idempotent");
}

#[test]
fn a_joined_secret_name_right_after_a_phrase_keeps_its_value_redacted() {
    for name in JOINED {
        for line in [
            format!("seed {PHRASE} {name}{VALUE}"),
            format!("Seed {} {}{VALUE}", capitalised(), name.to_uppercase()),
            format!("seed {}, {name}{VALUE}", phrase_words().join(", ")),
        ] {
            assert_clean(scrub_line, "scrub_line", &line, name);
            assert_clean(scrub_partner_line, "scrub_partner_line", &line, name);
        }
    }
}

/// Only a joined SECRET name is kept: a joined ordinary word, or a sentence
/// that simply ends after the phrase, leaves nothing of the phrase behind.
#[test]
fn a_joined_word_that_is_not_a_secret_name_is_not_kept() {
    for line in [
        format!("{PHRASE}-free: noted"),
        format!("loose {PHRASE}.done=1"),
        format!("loose {PHRASE}. Next sentence"),
    ] {
        let out = scrub_line(&line);
        assert!(!out.contains("adapt"), "{out}");
    }
}
