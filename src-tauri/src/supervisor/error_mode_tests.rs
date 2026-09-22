//! B15 defect 1: the loader hard-error suppression FEM had was THREAD-scoped,
//! so no partner it spawned ever inherited it.
//!
//! `loader_error_mode::suppress()` is taken on the throwaway spawn thread, and
//! it is correct for what it was written for: the WP6 case, where
//! `CreateProcess` itself blocks behind a modal box. But the dialog B15 reports
//! — "titan-edge.exe - Bad Image … goworkerd.dll … Error status 0xc0e90002" —
//! is raised by the CHILD's own loader resolving its static imports, long after
//! CreateProcess returned success to FEM. Nothing thread-local in FEM can reach
//! that. process.rs said so itself, in a test-only comment: "the hard-error
//! dialog is governed by the PROCESS error mode, which children inherit". No
//! shipped code ever set it.
//!
//! Whether an inherited `SEM_FAILCRITICALERRORS` genuinely converts a
//! code-integrity refusal into an exit code is a Win32 runtime question, not a
//! unit-test one — that is B15's vm_check 2. These tests pin the two things
//! source CAN decide: the mode FEM asks for, and that it is asked for at all,
//! once, at startup, with exactly one documented exemption.
//!
//! Deliberately NOT asserted here: the live `GetErrorMode()` value. The process
//! error mode is global state that `wp6_spawn_tests` already resets to 0 and
//! restores around its own run, and `cargo test` runs in parallel — a second
//! mutator would make both tests flaky rather than make either more true.

use super::HARD_ERROR_SUPPRESSION_MODE;

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX — the two boxes a partner's
/// loader can put on the user's desktop.
#[test]
fn the_requested_mode_suppresses_both_loader_dialogs() {
    const SEM_FAILCRITICALERRORS: u32 = 0x0001;
    const SEM_NOOPENFILEERRORBOX: u32 = 0x8000;
    assert_eq!(
        HARD_ERROR_SUPPRESSION_MODE & SEM_FAILCRITICALERRORS,
        SEM_FAILCRITICALERRORS,
        "the Bad Image / critical-error box must be suppressed"
    );
    assert_eq!(
        HARD_ERROR_SUPPRESSION_MODE & SEM_NOOPENFILEERRORBOX,
        SEM_NOOPENFILEERRORBOX,
        "the OpenFile error box must be suppressed"
    );
}

/// The defect in one line: a process-scope setter has to exist at all.
#[test]
fn the_spawner_suppresses_hard_errors_at_process_scope() {
    let code = code_only(include_str!("process.rs"));
    assert!(
        code.contains("fn suppress_process_hard_errors"),
        "a THREAD-scoped suppression cannot reach a child's own loader — a \
         process-scope setter, which children inherit, must exist"
    );
}

/// …and be called, once, before anything can start a partner.
#[test]
fn the_process_error_mode_is_set_at_startup() {
    let code = code_only(include_str!("../main.rs"));
    let call = code
        .find("suppress_process_hard_errors()")
        .expect("main's setup must set the process error mode");
    let first_partner_start = code.find("integration.start()").unwrap_or(code.len());
    assert!(
        call < first_partner_start,
        "the error mode must be set BEFORE the first partner can be started, \
         or the first spawn of the session can still raise a dialog"
    );
}

/// The thread-scoped guard is still correct for the WP6 case it was written
/// for, and `wp6_spawn_tests` depends on it. It must not be removed.
#[test]
fn the_existing_per_thread_guard_is_left_in_place() {
    let code = code_only(include_str!("process.rs"));
    assert!(
        code.contains("SetThreadErrorMode"),
        "the per-thread guard covers the CreateProcess-side WP6 case and stays"
    );
    assert!(
        code.contains("let _quiet = loader_error_mode::suppress();"),
        "the spawn thread must still take the per-thread guard"
    );
}

/// D-13: a process error mode is inherited by the WHOLE descendant tree.
/// Docker Desktop is a third-party app that owns its own UI and spawns an
/// engine/WSL tree, so it is the one launch that opts out. Olostep and
/// space-acres are FEM-managed partners and SHOULD inherit — that is precisely
/// the Done-when, "spawned partners never raise system modal dialogs".
#[test]
fn only_docker_desktop_opts_out_of_the_inherited_error_mode() {
    let exempt = "CREATE_DEFAULT_ERROR_MODE";
    for (name, src, expected) in [
        (
            "docker_manager.rs",
            include_str!("../integrations/docker_manager.rs"),
            true,
        ),
        ("aem.rs", include_str!("../integrations/aem.rs"), false),
        (
            "space_acres.rs",
            include_str!("../integrations/space_acres.rs"),
            false,
        ),
        ("process.rs", include_str!("process.rs"), false),
    ] {
        let found = code_only(src).contains(exempt);
        assert_eq!(
            found, expected,
            "{name}: expected opt-out={expected}, found {found}. Only Docker \
             Desktop may keep its own dialogs; FEM-managed partners must not."
        );
    }
}
