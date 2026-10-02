//! c4 BUG LOOP 5 — lens-1 RC14 NB: pins the two BL4-C mutants the first
//! tests let through: the raw elevation block re-added ahead of block_line
//! (which hides the shortfall again) and a negated gate inside block_line.

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_raw_block_never_reaches_the_card_on_its_own() {
    let code = code_only(include_str!("integration.rs"));
    let start = code
        .find("error: last_errors")
        .expect("the card error chain must exist");
    let end = start
        + code[start..]
            .find("unavailable_reason,")
            .expect("chain end");
    let chain = &code[start..end];
    assert!(
        !chain.contains(&format!("elevation{}.get(&id).cloned()", "_blocks")),
        "a raw elevation block in the chain hides D-C4-2's shortfall again:\n{chain}"
    );
}

#[test]
fn block_line_passes_the_gate_unchanged() {
    let code = code_only(include_str!("integration.rs"));
    let at = code
        .find("fn block_line(block: Option<&String>, fryvpn_live: bool) -> Option<String> {")
        .expect("block_line must exist");
    let body = &code[at..at + code[at..].find("\n}").expect("block_line must close")];
    assert!(
        body.contains("fryvpn_live)") && !body.contains("!fryvpn_live"),
        "block_line must hand the card's own gate through unchanged:\n{body}"
    );
}
