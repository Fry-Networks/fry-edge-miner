//! P0: the Sentinel unfunded-account card text.
use super::*;

/// Characterization of the text before the P0 copy rewrite.
#[test]
fn unfunded_message_keeps_the_awaiting_marker_and_the_address() {
    let m = SentinelIntegration::unfunded_message("sent1qqqqexample");
    assert!(m.contains("node account not funded"), "{m}");
    assert!(m.contains("sent1qqqqexample"), "{m}");
    assert!(crate::integrations::awaits_user_action(&m));
}
