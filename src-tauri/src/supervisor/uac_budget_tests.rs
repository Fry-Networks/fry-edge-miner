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

#[test]
fn a_human_answer_budget_is_materially_longer_than_a_cli_probe() {
    assert!(
        UAC_ANSWER_TIMEOUT > PROBE_TIMEOUT,
        "UAC_ANSWER_TIMEOUT ({UAC_ANSWER_TIMEOUT:?}) must be materially longer than the \
         generic probe timeout ({PROBE_TIMEOUT:?}) — it has to absorb however long a human \
         takes to notice and answer the consent dialog"
    );
}

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
