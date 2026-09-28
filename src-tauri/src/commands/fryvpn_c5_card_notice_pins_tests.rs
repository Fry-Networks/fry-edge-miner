//! Continuation #5: pins for the fryDVPN card-line survivors MUT found
//! (RC15-FV347-neg, RC15-FV347-none, RC15-RESP-a/b/c).
//!
//! The earlier pins read the source for one spelling each, and D-C4-2's
//! notice is never live in a unit test, so any respelling of the wrapper's
//! gate or source, or of `block_line`'s gate, passed. These drive the real
//! functions with a live notice. Each runs in a child process, because the
//! notice is process-global and other tests expect it absent.
//!
//! `get_integrations` itself needs a running Tauri app, so the card chain is
//! pinned by its shape: exactly one route for an elevation block, whatever
//! its spelling.

use crate::integrations::fryvpn::{set_funding_notice_for_test, with_heartbeat_shortfall};

const SCENARIO_VAR: &str = "FEM_C5_CARD_SCENARIO";
const DONE: &str = "FEM-C5-CARD-SCENARIO-DONE";
const NOTICE: &str = "fryDVPN is running, but this node's wallet cannot pay the 0.001 ALGO fee of \
                      its next heartbeat — send 0.000986 ALGO to ADDR.";

/// Run `scenario` in a child re-execution of this test binary.
fn run_scenario(scenario: &str) {
    let module = module_path!();
    let child_test = format!(
        "{}::child",
        module.split_once("::").map_or(module, |(_, rest)| rest)
    );
    let out = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            child_test.as_str(),
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SCENARIO_VAR, scenario)
        .output()
        .expect("re-execute this test binary");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("running 1 test"),
        "harness: `{child_test}` did not run exactly one test:\n{text}"
    );
    assert!(
        out.status.success() && text.contains(&format!("{DONE} {scenario}")),
        "scenario `{scenario}` failed in its child process:\n{text}"
    );
}

#[test]
fn the_wrapper_appends_the_live_notice_only_inside_the_card_gate() {
    run_scenario("wrapper");
}

#[test]
fn block_line_hands_the_card_gate_through_unchanged() {
    run_scenario("block_line");
}

#[test]
#[ignore = "scenario entry point: the tests above run it in a child process"]
fn child() {
    let Ok(scenario) = std::env::var(SCENARIO_VAR) else {
        return;
    };
    set_funding_notice_for_test(Some(NOTICE.to_string()));
    match scenario.as_str() {
        "wrapper" => wrapper(),
        "block_line" => block_line(),
        other => panic!("unknown scenario {other}"),
    }
    println!("{DONE} {scenario}");
}

fn wrapper() {
    let block = "Needs administrator approval — Retry".to_string();
    assert_eq!(
        with_heartbeat_shortfall(block.clone(), true),
        format!("{block} · {NOTICE}"),
        "inside the gate the live notice follows the block (RC15-FV347-none)"
    );
    assert_eq!(
        with_heartbeat_shortfall(block.clone(), false),
        block,
        "outside the gate the card shows only its block (RC15-FV347-neg)"
    );
}

fn block_line() {
    let block = "Needs administrator approval — Retry".to_string();
    assert_eq!(
        super::block_line(Some(&block), true),
        Some(format!("{block} · {NOTICE}")),
        "a live fryDVPN card carries the notice after its block (RC15-RESP-c)"
    );
    assert_eq!(
        super::block_line(Some(&block), false),
        Some(block.clone()),
        "a dead or disabled card shows only its block (RC15-RESP-c)"
    );
    assert_eq!(super::block_line(None, true), None);
}

fn squeeze(text: &str) -> String {
    text.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .flat_map(str::chars)
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// RC15-RESP-a/b: the raw block re-added above `block_line`, in any
/// spelling, is a second route for the block and hides the notice again.
#[test]
fn the_card_error_chain_has_one_route_for_an_elevation_block() {
    let code = squeeze(include_str!("integration.rs"));
    let start = code
        .find("pubasyncfnget_integrations(")
        .expect("get_integrations must exist");
    let end = start
        + code[start..]
            .find("Ok(statuses)")
            .expect("get_integrations must return its statuses");
    let body = &code[start..end];
    let chain_start = body
        .find("error:last_errors")
        .expect("the card error chain must exist");
    let chain_end = chain_start
        + body[chain_start..]
            .find("unavailable_reason,")
            .expect("the chain ends before unavailable_reason");
    let chain = &body[chain_start..chain_end];

    assert_eq!(
        chain.matches(".or_else(").count(),
        2,
        "last start error, then block_line, then the funding notice — no fourth route:\n{chain}"
    );
    let first = chain.find(".or_else(").unwrap();
    assert!(
        chain[first..].starts_with(".or_else(||block_line(elevation_blocks.get(&id),fryvpn_live))"),
        "the first fallback after the last start error is block_line:\n{chain}"
    );
    assert_eq!(
        body.matches("elevation_blocks").count(),
        2,
        "the block is read once and reaches the card only through block_line:\n{body}"
    );
    assert_eq!(body.matches("blocked_reasons()").count(), 1, "{body}");
}
