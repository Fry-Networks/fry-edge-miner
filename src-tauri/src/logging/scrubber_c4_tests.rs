//! Row 4 (continuation #4) — two leaks the run-#3 M6 cell found in an exported
//! debug bundle, pinned with its own inputs. Every value is synthetic: the
//! address, the miner keys and the tokens were made for these fixtures, and
//! the mnemonic is the first 25 words of the public BIP39 list.

use super::*;

/// Synthetic, checksum-valid Algorand address (not a real wallet).
const ADDR: &str = "YTC4NR6IZHFMXTGNZ3H5BUOS2PKNLVWX3DM5VW643XPN7YHB4LRUS2CHMA";

/// The 25 words of the phrase, in order.
const PHRASE: &str = "abandon ability able about above absent absorb abstract absurd abuse access accident account accuse achieve acid acoustic acquire across act action actor actress actual adapt";

/// Synthetic miner keys: FEM- + 32 hex, in the three cases a key can take.
const KEY_UPPER: &str = "FEM-C4C4C4C4DECAFBADC0FFEE0011223344";
const KEY_LOWER: &str = "FEM-c4c4c4c4decafbadc0ffee0011223344";
const KEY_MIXED: &str = "FEM-C4c4C4c4DecafBadC0ffee0011223344";

/// The run-#3 M6 corpus (`c3-m6/w11/corpus-injected.log`), line for line,
/// with its wallet address swapped for the synthetic one.
fn m6_corpus() -> Vec<String> {
    vec![
        format!("2026-09-24T07:00:00.000000Z  INFO femqa_corpus: wallet address {ADDR}"),
        format!("2026-09-24T07:00:01.000000Z  INFO femqa_corpus: mnemonic: {PHRASE}"),
        format!("2026-09-24T07:00:02.000000Z  INFO femqa_corpus: loose {PHRASE} trailing"),
        "2026-09-24T07:00:03.000000Z  INFO femqa_corpus: WG public key: c3QAsynthWGkeyAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string(),
        "2026-09-24T07:00:04.000000Z  INFO femqa_corpus: starting agent --user.token=femqaC3SynthTokenValue123 --flag".to_string(),
        r#"2026-09-24T07:00:05.000000Z  INFO femqa_corpus: {"api_key":"femqaC3SynthApiKey456","ok":true}"#.to_string(),
        "2026-09-24T07:00:06.000000Z  INFO femqa_corpus: Authorization: Bearer femqaC3SynthBearer789".to_string(),
        "2026-09-24T07:00:07.000000Z  INFO femqa_corpus: CONTROL_UNSECRET marker femqa-c3-corpus-control-line".to_string(),
    ]
}

/// The two lines of the M6 bundle's `fem.log` that carried the device's
/// miner key in plain text — uppercase hex, which `redact_serial` (lowercase
/// only) never matched.
fn m6_bundle_lines(key: &str) -> [String; 2] {
    [
        format!("2026-09-24T06:43:31.410820Z  INFO fry_edge_miner::commands::device: Device auto-migrated to per-device token miner_key={key}"),
        format!("2026-09-24T06:45:05.461118Z  INFO fry_edge_miner::commands::device: App version changed since last report — sending immediate heartbeat + lease action miner_key={key} from=\"0.4.29\" to=\"0.4.34\""),
    ]
}

/// The 32 hex characters, which must not survive in any case.
fn key_hex(key: &str) -> String {
    key.trim_start_matches("FEM-").to_ascii_lowercase()
}

#[test]
fn the_miner_key_in_the_m6_bundle_lines_is_redacted_in_any_case() {
    for key in [KEY_UPPER, KEY_LOWER, KEY_MIXED] {
        for line in m6_bundle_lines(key) {
            let out = scrub_line(&line);
            assert!(
                !out.to_ascii_lowercase().contains(&key_hex(key)),
                "the miner key reached the bundle: {out}"
            );
            assert!(
                out.contains("miner_key=FEM-"),
                "only the key's value is redacted, not the field: {out}"
            );
            assert_eq!(scrub_line(&out), out, "scrub_line must stay idempotent");
        }
    }
    // The rest of the line is diagnostic and must survive.
    let out = scrub_line(&m6_bundle_lines(KEY_UPPER)[1]);
    assert!(
        out.contains(r#"from="0.4.29" to="0.4.34""#),
        "over-redacted: {out}"
    );
}

#[test]
fn every_word_of_a_prose_prefixed_phrase_is_redacted_including_its_tail() {
    let words: Vec<&str> = PHRASE.split(' ').collect();
    assert_eq!(words.len(), 25, "fixture: a 25-word phrase");
    let lowercase = format!("loose {PHRASE} trailing");
    let mixed_case = format!(
        "Loose {} trailing",
        words
            .iter()
            .map(|w| format!("{}{}", w[..1].to_ascii_uppercase(), &w[1..]))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let comma_separated = format!("loose {} trailing", words.join(", "));
    for line in [lowercase, mixed_case, comma_separated] {
        let out = scrub_line(&line);
        assert!(out.contains("[MNEMONIC]"), "{out}");
        for word in &words {
            assert!(
                !out.to_ascii_lowercase()
                    .split(|c: char| !c.is_ascii_alphabetic())
                    .any(|w| w == *word),
                "the phrase word {word:?} survived: {out}"
            );
        }
        assert_eq!(scrub_line(&out), out, "scrub_line must stay idempotent");
    }
}

/// A prose-prefixed phrase is exactly what a partner's own log can print, and
/// the write-time scrubber uses the same two mnemonic rules.
#[test]
fn a_partner_log_line_loses_the_tail_word_too() {
    let out = scrub_partner_line(&format!("loose {PHRASE} trailing"));
    assert!(!out.contains("adapt"), "{out}");
}

#[test]
fn the_m6_corpus_scrubs_clean_and_keeps_its_address_redacted() {
    let out: Vec<String> = m6_corpus().iter().map(|l| scrub_line(l)).collect();
    let all = out.join("\n");
    assert!(
        out[0].contains("YTC4…CHMA") && !all.contains(ADDR),
        "the address is still shown only as first4…last4: {}",
        out[0]
    );
    assert!(
        !all.contains("adapt"),
        "the tail word of the prose-prefixed phrase survived:\n{all}"
    );
    for secret in [
        "abandon",
        "c3QAsynthWGkey",
        "femqaC3SynthTokenValue123",
        "femqaC3SynthApiKey456",
        "femqaC3SynthBearer789",
    ] {
        assert!(!all.contains(secret), "{secret} survived:\n{all}");
    }
    assert!(
        out[7].contains("CONTROL_UNSECRET marker") && out[7].ends_with("-c3-corpus-control-line"),
        "a line with nothing secret must survive: {}",
        out[7]
    );
}
