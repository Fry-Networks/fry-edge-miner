//! FAIL-11 (row 6): at 5b7c8f5, `run_hardening_elevated` had exactly two
//! callers — the boot pass (main.rs) and the pre-update re-assert
//! (updater_auto.rs) — and BOTH pass `ElevationTrigger::Automatic`, which the
//! gate refuses before any UAC prompt appears. No caller anywhere in the
//! tree passed `UserClick`, so a user who actually wanted hardening applied
//! (after a decline, or just proactively) had no way to ask for it.
//!
//! `retry_hardening` (this module's sibling, `hardening.rs`) is the new,
//! ONLY caller that passes `UserClick`. These tests pin that: the new call
//! site really does use `UserClick` (source-scan of the exact file this
//! command lives in — the same technique `docker_boot_contract_tests.rs` and
//! `bug10_elevation_hygiene_tests` already use in this crate), the two
//! pre-existing automatic call sites are untouched, and the underlying
//! primitive (`run_hardening_elevated`/`elevation_gate::run_elevated`, both
//! unmodified by this fix) really does thread `UserClick` through to the
//! gate's per-purpose attempt tracking end to end.

const HARDENING_CMD_SRC: &str = include_str!("hardening.rs");
const MAIN_SRC: &str = include_str!("../main.rs");
const UPDATER_SRC: &str = include_str!("../updater_auto.rs");

/// Strip line comments so prose can never satisfy an assertion — the same
/// guard `docker_boot_contract_tests.rs` and `bug10_elevation_hygiene_tests`
/// use, for the same reason.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// THE fix: the new command's call to `run_hardening_elevated` passes
/// `UserClick`, not `Automatic`.
#[test]
fn retry_hardening_calls_run_hardening_elevated_with_user_click() {
    let code = code_only(HARDENING_CMD_SRC);
    let at = code
        .find("run_hardening_elevated(")
        .expect("retry_hardening must call run_hardening_elevated");
    let window = &code[at..(at + 400).min(code.len())];
    assert!(
        window.contains("ElevationTrigger::UserClick"),
        "retry_hardening's call to run_hardening_elevated does not pass \
         UserClick: {window}"
    );
    assert!(
        !window.contains("ElevationTrigger::Automatic"),
        "retry_hardening must never pass Automatic — that would make it \
         indistinguishable from the boot pass and the gate would refuse it \
         silently: {window}"
    );
}

/// Do not change the boot pass's trigger, or the updater's. Both must still
/// be `Automatic` after this fix.
#[test]
fn the_boot_pass_and_the_updater_pre_update_reassert_are_unchanged_at_automatic() {
    for (name, src) in [("main.rs", MAIN_SRC), ("updater_auto.rs", UPDATER_SRC)] {
        let code = code_only(src);
        let at = code
            .find("run_hardening_elevated(")
            .unwrap_or_else(|| panic!("{name} must still call run_hardening_elevated"));
        let window = &code[at..(at + 500).min(code.len())];
        assert!(
            window.contains("ElevationTrigger::Automatic"),
            "{name}'s hardening call site must stay Automatic: {window}"
        );
    }
}

/// Non-vacuity / mechanism proof, independent of the source-scan above: the
/// PRIMITIVE `run_hardening_elevated`/`elevation_gate::run_elevated` (both
/// unmodified — the bug was a missing caller, not broken plumbing) really
/// does thread `UserClick` through to the gate's per-purpose attempt
/// tracking. `Automatic` returns before ever touching that map, so only a
/// UserClick call can make a REPEAT of the same key come back
/// `AlreadyAttempted`.
#[test]
fn user_click_hardening_reaches_the_gate_attempt_tracking() {
    use crate::elevation_gate::ElevationSkipped;

    let tmp = tempfile::tempdir().expect("tempdir");
    let install_dir = tmp.path();
    let frynode = install_dir.join("frynode.exe");
    let exe_names = ["fry-edge-miner.exe", "frynode.exe"];
    // Unique per test run so this test is never accidentally already-spent
    // by an earlier run's key (tests in this binary run in the same
    // process, and the gate's attempt map is process-global).
    let key = format!(
        "FAIL-11-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );

    let first = crate::security_setup::run_hardening_elevated(
        install_dir,
        &exe_names,
        &frynode,
        &key,
        crate::elevation_gate::ElevationTrigger::UserClick,
    );
    assert_ne!(
        first,
        Err(ElevationSkipped::AlreadyAttempted),
        "a fresh key's first UserClick attempt must never be AlreadyAttempted: {first:?}"
    );

    let second = crate::security_setup::run_hardening_elevated(
        install_dir,
        &exe_names,
        &frynode,
        &key,
        crate::elevation_gate::ElevationTrigger::UserClick,
    );
    assert_eq!(
        second,
        Err(ElevationSkipped::AlreadyAttempted),
        "a repeat UserClick with the SAME key must be suppressed as \
         already-attempted — proving the first call really did reach the \
         gate's attempt tracking, which only a UserClick trigger touches"
    );
}
