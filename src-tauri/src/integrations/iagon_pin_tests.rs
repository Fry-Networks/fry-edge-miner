//! G4 finding 20 — Iagon's pin failed OPEN.
//!
//! A digest mismatch was downgraded to a warning and any file over 50 MB was
//! accepted, chmod'd executable and spawned on every launch thereafter. A size
//! check is not a weaker digest; it is satisfied by exactly the artifacts a pin
//! exists to reject — a CDN-cached wrong asset, a partially mirrored release, a
//! substituted binary. And because install() returned early on mere existence,
//! nothing ever re-downloaded or re-hashed it: once wrong, trusted forever.

use super::*;

/// Needles assembled at runtime so these guards cannot match their own text.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

const IAGON_SRC: &str = include_str!("iagon.rs");

#[test]
fn the_pin_is_a_full_sha256() {
    assert_eq!(IAGON_SHA256.len(), 64, "{IAGON_SHA256}");
    assert!(IAGON_SHA256.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(
        IAGON_SHA256.chars().any(|c| c != '0'),
        "an all-zero pin is a placeholder, not a pin"
    );
}

/// The specific override that made the pin advisory.
#[test]
fn a_digest_mismatch_is_no_longer_excused_by_a_size_check() {
    let code = code_only(IAGON_SRC);

    let size_excuse = format!("{}_000_000", "50");
    assert!(
        !code.contains(&size_excuse),
        "a size threshold still stands in for the digest:\n{code}"
    );
    let proceed = format!("proceeding with {}", "caution");
    assert!(
        !code.contains(&proceed),
        "the fail-open branch is still present"
    );
}

/// install() must not trust a file for merely existing, and must fail closed.
#[test]
fn install_re_verifies_an_existing_binary_and_fails_closed() {
    let code = code_only(IAGON_SRC);
    let at = code
        .find(&format!("async fn install(&{}) -> Result<()> {{", "self"))
        .expect("install must exist");
    let end = code[at..]
        .find(&format!("\n    async fn start(&{})", "self"))
        .map(|e| at + e)
        .expect("start follows install");
    let body = &code[at..end];

    let verify = format!("Self::verify{}(", "_binary");
    let quarantine = format!("Self::quarantine{}(", "_untrusted");
    assert!(
        body.matches(&verify).count() >= 2,
        "install must verify BOTH an existing binary and a freshly downloaded one:\n{body}"
    );
    assert!(
        body.contains(&quarantine),
        "a binary that fails its pin must be quarantined, not left in place:\n{body}"
    );
    assert!(
        body.contains(&format!("anyhow::{}!(", "bail")),
        "install must fail closed on a digest mismatch:\n{body}"
    );
}

/// B15's "before every spawn", for the integration the clause was written for.
#[test]
fn start_verifies_the_binary_before_every_spawn() {
    let code = code_only(IAGON_SRC);
    let at = code
        .find(&format!("async fn start(&{}) -> Result<()> {{", "self"))
        .expect("start must exist");
    let end = code[at..]
        .find("\n    async fn ")
        .map(|e| at + e)
        .unwrap_or(code.len());
    let body = &code[at..end];

    let verify = format!("Self::verify{}(", "_binary");
    let spawn = format!("start_integration{}(", "_with_env");
    let v = body
        .find(&verify)
        .expect("start must verify the binary before spawning it");
    if let Some(s) = body.find(&spawn) {
        assert!(v < s, "the spawn happens before verification:\n{body}");
    }
    assert!(
        body.contains(&format!("self.{}().await?", "install")),
        "a mismatch at spawn time must trigger an automatic repair:\n{body}"
    );
}
