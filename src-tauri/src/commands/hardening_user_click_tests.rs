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
///
/// BUG LOOP 2 (NB): `UserClick` makes `run_elevated` actually RUN the
/// closure — on Windows that spawns the REAL elevated PowerShell (`-Verb
/// RunAs -Wait`), which raises a genuine UAC prompt and, if approved,
/// applies real Defender exclusions and rewrites the real FEM-FryNode
/// firewall rule on whatever machine runs `cargo test`, including the
/// windows-latest release runner. `#[cfg(not(windows))]` keeps this test —
/// and its non-vacuous mechanism proof — on Linux CI (the actual gate for
/// this crate's unit tests), where "powershell" is simply absent and the
/// closure fails harmlessly with `NotFound`, while making it impossible for
/// this file to ever run real elevation on any Windows machine, dev or CI.
#[cfg(not(windows))]
#[test]
fn user_click_hardening_reaches_the_gate_attempt_tracking() {
    use crate::elevation_gate::ElevationSkipped;

    // `elevation_gate`'s blocked-reasons map is keyed by PURPOSE only (not
    // by attempt key), so this must serialize against every other test in
    // this file that touches "hardening" — see HARDENING_PURPOSE_TEST_LOCK.
    let _guard = HARDENING_PURPOSE_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());

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

// -----------------------------------------------------------------------
// BUG LOOP 2, item 1 (BLOCKING): the boot pass's Automatic refusal
// publishes into the gate within microseconds of setup — long before the
// webview loads, React mounts, or the live `elevation-required` listener
// registers. `get_hardening_status` is the mount-time pull that recovers
// that state; these pin it exists, is registered, and behaves correctly.
// -----------------------------------------------------------------------

const MAIN_SRC_FULL: &str = include_str!("../main.rs");

/// Extract exactly ONE function's body (from its signature's opening `{` to
/// its OWN matching closing `}`, by brace balance) — the same technique used
/// elsewhere in this crate's source-scan tests, duplicated here since each
/// `#[path]` test file is self-contained.
fn fn_body<'a>(code: &'a str, signature_needle: &str) -> &'a str {
    let at = code
        .find(signature_needle)
        .unwrap_or_else(|| panic!("signature not found: {signature_needle}"));
    let open = at
        + signature_needle
            .rfind('{')
            .expect("signature needle must end at the opening brace");
    let bytes = code.as_bytes();
    let mut depth = 0i32;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &code[at..=i];
                }
            }
            _ => {}
        }
        i += 1;
    }
    panic!("no matching closing brace for: {signature_needle}");
}

/// THE fix: `get_hardening_status` exists and reads
/// `elevation_gate::blocked_reasons()`.
#[test]
fn get_hardening_status_reads_the_gates_blocked_reasons() {
    let code = code_only(HARDENING_CMD_SRC);
    let body = fn_body(
        &code,
        "pub async fn get_hardening_status() -> Result<Option<String>, String> {",
    );
    assert!(
        body.contains("blocked_reasons()") && body.contains("\"hardening\""),
        "get_hardening_status must read elevation_gate::blocked_reasons()[\"hardening\"]: {body}"
    );
}

/// The command must be registered in main.rs's generate_handler! list, or
/// the frontend can never call it on mount.
#[test]
fn get_hardening_status_is_registered_in_generate_handler() {
    let main_code = code_only(MAIN_SRC_FULL);
    let gh_at = main_code
        .find("generate_handler![")
        .expect("control: generate_handler! must exist");
    let gh_end = main_code[gh_at..]
        .find(']')
        .map(|e| gh_at + e)
        .expect("control: generate_handler![ must close");
    let gh_block = &main_code[gh_at..gh_end];
    assert!(
        gh_block.contains("commands::hardening::get_hardening_status,"),
        "get_hardening_status is not registered in generate_handler!: {gh_block}"
    );
}

/// Behavioural, no AppState needed (`get_hardening_status` is state-free):
/// once the gate holds a "hardening" block, the command surfaces the exact
/// reason string — proving the mount-time pull actually recovers a block
/// that happened before any listener existed, not just that the two pieces
/// of text are present.
///
/// Serialized against every other test in this file that touches the
/// "hardening" purpose (`clear_blocked`/`blocked_reasons` are keyed by
/// purpose only, not by attempt key, and are process-global) — see
/// `HARDENING_PURPOSE_TEST_LOCK`.
#[tokio::test]
async fn get_hardening_status_surfaces_a_currently_published_block() {
    let _guard = HARDENING_PURPOSE_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    crate::elevation_gate::clear_blocked("hardening");
    assert_eq!(
        super::get_hardening_status().await,
        Ok(None),
        "control: nothing blocked yet"
    );

    // A real Automatic call, exactly like the boot pass's, publishes the
    // block as a side effect (elevation_gate::run_elevated -> publish_block)
    // without ever running the closure.
    let published = crate::elevation_gate::run_elevated(
        "hardening",
        "bl2-status-test",
        crate::elevation_gate::ElevationTrigger::Automatic,
        || -> anyhow::Result<()> { unreachable!("Automatic must never run the closure") },
    );
    assert!(matches!(
        published,
        Err(crate::elevation_gate::ElevationSkipped::NeedsApproval)
    ));

    assert_eq!(
        super::get_hardening_status().await,
        Ok(Some(
            crate::elevation_gate::NEEDS_APPROVAL_MESSAGE.to_string()
        )),
        "get_hardening_status must surface the gate's currently published block"
    );

    crate::elevation_gate::clear_blocked("hardening");
}

// -----------------------------------------------------------------------
// BUG LOOP 2, item 2 (NB): retry_hardening must re-arm the gate on every
// gesture, or a declined/timed-out UAC spends the version's one attempt
// forever and every later Retry click this run is a silent no-op.
// -----------------------------------------------------------------------

/// THE fix: retry_hardening calls `clear_blocked("hardening")` BEFORE
/// `run_hardening_elevated`.
#[test]
fn retry_hardening_rearms_the_gate_before_running() {
    let code = code_only(HARDENING_CMD_SRC);
    let body = fn_body(
        &code,
        "pub async fn retry_hardening(state: tauri::State<'_, crate::AppState>) -> Result<(), String> {",
    );
    let clear_at = body
        .find("clear_blocked(\"hardening\")")
        .unwrap_or_else(|| {
            panic!("retry_hardening must call clear_blocked(\"hardening\"): {body}")
        });
    let run_at = body.find("run_hardening_elevated(").unwrap_or_else(|| {
        panic!("retry_hardening must still call run_hardening_elevated: {body}")
    });
    assert!(
        clear_at < run_at,
        "clear_blocked must run BEFORE run_hardening_elevated, or the SAME \
         gesture's own attempt could be cleared out from under it: {body}"
    );
}

/// Behavioural mechanism proof (non-discriminating on its own — clear_blocked
/// itself is pre-existing, correct code; paired with the structural test
/// above, which is what's new): clearing "hardening" between two UserClick
/// attempts with the SAME key un-suppresses the second one.
#[test]
fn clearing_blocked_before_a_repeat_user_click_avoids_already_attempted() {
    use crate::elevation_gate::ElevationSkipped;

    let _guard = HARDENING_PURPOSE_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let tmp = tempfile::tempdir().expect("tempdir");
    let install_dir = tmp.path();
    let frynode = install_dir.join("frynode.exe");
    let exe_names = ["fry-edge-miner.exe", "frynode.exe"];
    let key = "bl2-rearm-test";

    let first = crate::security_setup::run_hardening_elevated(
        install_dir,
        &exe_names,
        &frynode,
        key,
        crate::elevation_gate::ElevationTrigger::UserClick,
    );
    assert_ne!(first, Err(ElevationSkipped::AlreadyAttempted));

    crate::elevation_gate::clear_blocked("hardening");

    let second = crate::security_setup::run_hardening_elevated(
        install_dir,
        &exe_names,
        &frynode,
        key,
        crate::elevation_gate::ElevationTrigger::UserClick,
    );
    assert_ne!(
        second,
        Err(ElevationSkipped::AlreadyAttempted),
        "clear_blocked(\"hardening\") must re-arm the SAME key, or \
         retry_hardening calling it would not actually help: {second:?}"
    );
}

/// Process-global: `elevation_gate`'s blocked-reasons map and attempt
/// tracking are keyed by PURPOSE only (not by attempt key), so every test in
/// this file that touches the "hardening" purpose must serialize against
/// every other one — `cargo test` runs tests in parallel threads by default.
static HARDENING_PURPOSE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
