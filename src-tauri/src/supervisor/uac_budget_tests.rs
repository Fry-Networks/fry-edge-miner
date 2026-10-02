//! B3 defect 5: every wrapper that puts a UAC prompt on screen must be bounded
//! by a HUMAN-answer budget, not by the generic 20s CLI-probe budget.
//!
//! `output_bounded` kills the requesting (unelevated) PowerShell at its
//! deadline. The consent dialog is created by the AppInfo service via
//! `ShellExecuteEx` — security_setup.rs says so in its own words — so killing
//! the parent cannot dismiss it. At `PROBE_TIMEOUT` that means anyone who takes
//! longer than 20 seconds to answer can never grant the rule, and the next tick
//! asks again on top of the dialog that is still up.
//!
//! `titan.rs` already made exactly this argument for its own elevation and
//! guarded it with a test (`VC_REDIST_INSTALL_TIMEOUT > PROBE_TIMEOUT`);
//! firewall.rs and security_setup.rs were never given the same treatment. This
//! is that guard, generalised over every UAC-bearing wrapper in the tree.

use super::{PROBE_TIMEOUT, UAC_ANSWER_TIMEOUT};

const SOURCES: [(&str, &str); 2] = [
    ("firewall.rs", include_str!("../integrations/firewall.rs")),
    ("security_setup.rs", include_str!("../security_setup.rs")),
];

/// Strip line comments so prose can never satisfy an assertion — the same
/// guard `bug10_elevation_hygiene_tests` uses, and for the same reason.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The argument list of the first `output_bounded(` call at or after `from`.
fn next_bounded_argument(code: &str, from: usize) -> Option<String> {
    let needle = "output_bounded(";
    let start = code[from..].find(needle)? + from + needle.len();
    let end = code[start..].find(')')? + start;
    Some(code[start..end].to_string())
}

/// Both operands are compile-time constants, so a `#[test]` asserting this
/// could never fail at RUN time — it would only ever restate the source. As a
/// `const` assertion it fails the BUILD instead, which is what a relationship
/// between two constants deserves.
const _: () = assert!(
    UAC_ANSWER_TIMEOUT.as_secs() > PROBE_TIMEOUT.as_secs(),
    "UAC_ANSWER_TIMEOUT must be materially longer than the generic probe timeout — it has to \
     absorb however long a human takes to notice and answer the consent dialog"
);

#[test]
fn every_uac_bearing_wrapper_gets_a_human_answer_budget() {
    let mut checked = 0usize;
    for (name, src) in SOURCES {
        let code = code_only(src);
        let mut cursor = 0usize;
        while let Some(hit) = code[cursor..].find("-Verb RunAs") {
            let at = cursor + hit;
            cursor = at + "-Verb RunAs".len();
            // A `-Verb RunAs` with no `output_bounded` after it in this file is
            // not a wrapper (e.g. the hygiene test's own literal).
            let Some(arg) = next_bounded_argument(&code, at) else {
                continue;
            };
            checked += 1;
            assert!(
                !arg.contains("PROBE_TIMEOUT"),
                "{name}: the elevation at byte {at} is bounded by `{arg}` — the generic 20s \
                 CLI-probe budget. It puts a UAC prompt on screen, so it needs \
                 UAC_ANSWER_TIMEOUT; PROBE_TIMEOUT kills the requesting PowerShell while the \
                 consent dialog is still up."
            );
            assert!(
                arg.contains("UAC_ANSWER_TIMEOUT"),
                "{name}: the elevation at byte {at} is bounded by `{arg}` — every UAC-bearing \
                 wrapper must use the named human-answer budget so the intent is explicit."
            );
        }
    }
    assert!(
        checked >= 3,
        "expected at least the three known UAC-bearing wrappers (firewall::ensure_program_rules, \
         firewall::delete_rules, security_setup::run_hardening_elevated), found {checked}"
    );
}

/// G4 finding 3. Raising the budget to 180s is only safe if the elevation does
/// not run ON the async task.
///
/// `firewall::ensure_program_rules` is a plain synchronous fn, and
/// `output_bounded` blocks the calling OS thread in a try_wait/sleep loop. It is
/// called directly from `async fn start_inner`, whose `start_for_user` is
/// awaited under `tokio::time::timeout(TOGGLE_STEP_TIMEOUT /* 60s */, ..)`. A
/// `tokio::time::timeout` can only fire when its task is polled, and that task
/// is parked inside the blocking call — so at 20s the toggle's bound still held
/// with room to spare, and at 180s it does not hold at all: the worker thread
/// is pinned for three minutes, the card sits on the Installing spinner, and
/// when the call finally returns the timeout yields Ok, so the 60s error
/// message the code promises is never produced.
///
/// `titan.rs` already wraps the identical `-Verb RunAs` + `output_bounded`
/// pattern in `spawn_blocking`, and `mysterium_lan_check.rs` moves a blocking
/// probe off the async worker for exactly this reason. The two firewall call
/// sites must do the same. This is RED until they do — the constant is mine,
/// the call sites are not.
#[test]
fn the_human_answer_budget_never_blocks_an_async_task() {
    let offload = format!("spawn{}blocking", "_");
    for (name, src) in [
        ("fryvpn.rs", include_str!("../integrations/fryvpn.rs")),
        ("aem.rs", include_str!("../integrations/aem.rs")),
    ] {
        let code = code_only(src);
        let mut cursor = 0usize;
        while let Some(hit) = code[cursor..].find("ensure_program_rules(") {
            let at = cursor + hit;
            cursor = at + 1;
            // The offload has to be established before the call, so look back
            // over the enclosing statement rather than the whole file.
            let window_start = at.saturating_sub(400);
            assert!(
                code[window_start..at].contains(&offload),
                "{name}: the elevation at byte {at} runs on the async task. At \
                 UAC_ANSWER_TIMEOUT ({UAC_ANSWER_TIMEOUT:?}) that pins a tokio \
                 worker for three minutes and silently voids the 60s toggle \
                 bound the card's error handling is built on — move it off the \
                 task with {offload}, as titan.rs already does."
            );
        }
    }
}
