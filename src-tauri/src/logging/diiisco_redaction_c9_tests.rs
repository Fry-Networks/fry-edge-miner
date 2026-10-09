//! C9: the Diiisco bearer token is redacted in both log sinks. Synthetic values only.
use super::{scrub_line, scrub_partner_line};

#[test]
fn c9_diiisco_bearer_token_is_redacted_in_both_sinks() {
    for line in [
        r#"2026-10-05T10:00:00Z DBG credentials body {"miner_key":"FEM-C9TEST","diiisco_bearer_token":"c9-synthetic-diiisco"}"#,
        "2026-10-05T10:00:00Z DBG creds diiisco_bearer_token=c9-synthetic-diiisco ok",
    ] {
        let full = scrub_line(line);
        assert!(
            !full.contains("c9-synthetic-diiisco"),
            "bundle leak: {full}"
        );
        let partner = scrub_partner_line(line);
        assert!(
            !partner.contains("c9-synthetic-diiisco"),
            "partner leak: {partner}"
        );
    }
}
