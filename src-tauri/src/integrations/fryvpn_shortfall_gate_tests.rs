//! c4 BUG LOOP 5 — lens-1 RC14 NB: the BL4-C gate had only a vacuous
//! behavioural proof (the process-wide notice is None in unit tests, so
//! `if !applies` passed every test). The notice source is injected here.

#[test]
fn the_shortfall_is_appended_only_when_the_card_gate_applies() {
    let block = "Needs administrator approval - Retry".to_string();
    let live = || Some("send 0.001000 ALGO to ADDR".to_string());
    let on = super::with_heartbeat_shortfall_from(block.clone(), true, live);
    assert!(
        on.starts_with(&block) && on.contains("send 0.001000 ALGO to ADDR"),
        "{on}"
    );
    let off = super::with_heartbeat_shortfall_from(block.clone(), false, live);
    assert_eq!(
        off, block,
        "a dead or disabled node's card shows only its real state"
    );
    let none = super::with_heartbeat_shortfall_from(block.clone(), true, || None);
    assert_eq!(none, block);
}

#[test]
fn the_notice_is_not_even_read_outside_the_gate() {
    let block = "b".to_string();
    let _ = super::with_heartbeat_shortfall_from(block, false, || {
        panic!("the notice must not be read when the gate does not apply")
    });
}
