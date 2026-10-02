//! c4 BUG LOOP 4 (BL4-C) — D-C4-2's shortfall notice must reach the card.
//!
//! `get_integrations` built the card error as last start error ->
//! suppressed-elevation block -> fryDVPN funding notice, each hiding the
//! next. On any machine where FEM-FryNode's firewall rule is missing (every
//! install whose automatic elevation the gate suppresses) the fryvpn card
//! showed only "Needs administrator approval - Retry" for as long as the rule
//! was missing, and the ONE shortfall notice D-C4-2 requires ("send
//! <shortfall> ALGO to <node address>") never rendered. RC13 evidence:
//! c4-fdvpn-w10 case 4 — heartbeats rejected, fem.log logged the notice once,
//! the card showed only the administrator line. The block keeps its lead; the
//! live shortfall is appended to it.

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Wired: the elevation block reaches the card through `block_line`, gated on
/// the same fryDVPN / enabled / Healthy condition as the notice itself.
#[test]
fn a_suppressed_elevation_carries_the_shortfall_onto_the_card() {
    let code = code_only(include_str!("integration.rs"));
    assert!(
        code.contains("let fryvpn_live = id == \"fryvpn\" && enabled && healthy;"),
        "the shortfall must keep the notice's own gate (fryDVPN, enabled, Healthy)"
    );
    let start = code
        .find("error: last_errors")
        .expect("the card error chain must exist");
    let end = start
        + code[start..]
            .find("unavailable_reason,")
            .expect("the chain must end before unavailable_reason");
    let chain = &code[start..end];
    assert!(
        chain.contains(&format!(
            "block_line(elevation{}.get(&id), fryvpn_live)",
            "_blocks"
        )),
        "a suppressed elevation still reaches the card on its own and hides the shortfall:\n{chain}"
    );
}

#[test]
fn the_block_leads_and_the_shortfall_follows_on_one_line() {
    use crate::integrations::fryvpn::join_shortfall;
    let block = "Needs administrator approval - Retry".to_string();
    let line = join_shortfall(
        block.clone(),
        Some("send 0.001000 ALGO to ADDR".to_string()),
    );
    assert!(line.contains("send 0.001000 ALGO to ADDR"), "{line}");
    assert!(
        line.starts_with("Needs administrator approval - Retry"),
        "the block keeps its lead: {line}"
    );
    assert!(
        !line.contains('\n'),
        "one line, or the card drops half: {line}"
    );
    assert_eq!(join_shortfall(block.clone(), None), block);
}

#[test]
fn a_card_outside_the_notice_gate_shows_only_its_block() {
    let block = "Needs administrator approval - Retry".to_string();
    assert_eq!(
        crate::integrations::fryvpn::with_heartbeat_shortfall(block.clone(), false),
        block
    );
    assert_eq!(super::block_line(None, true), None);
}
