//! c4 BUG LOOP 4 (BL4-A) — FAIL-13 on every automatic path, not only Pawns'.
//!
//! FAIL-13's acceptance: no Docker Desktop download without an explicit user
//! gesture. The first fix moved `PawnsIntegration::start()` onto
//! `ensure_docker_no_install()`, but `ensure_docker()` — the Automatic default
//! that the boot recovery pass and supervisor restarts reach through
//! sentinel's, diiisco's and pawns' `install()`/`start()` — still took the
//! downloading branch. On RC13 (cell c4-b3-uac, UAC-on W11, every integration
//! enabled, nobody at the keyboard) FEM fetched the 625 MB installer about
//! nine times in thirty minutes, and the elevation gate then refused the
//! install every time. The gate only ever guarded the elevation AFTER the
//! download; the download itself must need the gesture.

/// Comments stripped, so these guards read code and not prose.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

const DOCKER_SRC: &str = include_str!("docker_manager.rs");

/// The text of `ensure_docker_core`'s `NotInstalled` arm up to the download.
fn not_installed_guard() -> String {
    let code = code_only(DOCKER_SRC);
    let core = code
        .find(&format!("async fn ensure_docker{}(", "_core"))
        .expect("ensure_docker_core must exist");
    let arm = code[core..]
        .find("DockerStatus::NotInstalled =>")
        .map(|a| core + a)
        .expect("ensure_docker_core must still handle NotInstalled");
    let download = code[arm..]
        .find(&format!("download_docker{}()", "_installer"))
        .map(|d| arm + d)
        .expect("a user gesture must still be able to download the installer");
    code[arm..download].to_string()
}

/// The refusal sits BEFORE the network fetch, is decided by the caller's
/// trigger, and actually returns, so an Automatic caller cannot reach the
/// download at all. The guard statement is matched whole: a guard that is
/// present but can never fire (`if false && …`) must not pass.
#[test]
fn the_installer_download_is_decided_by_the_callers_trigger() {
    let guard = not_installed_guard();
    let stmt = format!("if !may_fetch{}(trigger) {{", "_docker_installer");
    let at = guard.find(&stmt).unwrap_or_else(|| {
        panic!(
            "the NotInstalled arm downloads without refusing an Automatic caller first, so the \
             boot pass and supervisor restarts fetch the installer with nobody at the \
             keyboard:\n{guard}"
        )
    });
    let block = &guard[at + stmt.len()..];
    let block = &block[..block.find('}').expect("the guard block must close")];
    assert!(
        block.contains("bail!"),
        "the trigger refusal must return before the download starts:\n{block}"
    );
}

/// The decision itself, for both triggers the callers can pass.
#[test]
fn only_a_user_click_may_fetch_the_installer() {
    use crate::elevation_gate::ElevationTrigger::{Automatic, UserClick};
    assert!(!super::may_fetch_docker_installer(Automatic));
    assert!(super::may_fetch_docker_installer(UserClick));
}
