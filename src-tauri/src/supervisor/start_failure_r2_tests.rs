//! D1 round 2: a start-failure record must describe the CURRENT attempt's
//! startup, never a long-running or an older child. Store is process-global;
//! every test uses its own id.

use super::*;
use std::time::Duration;

const ACCESS_VIOLATION: i32 = -1073741819; // 0xC0000005

#[test]
fn an_exit_inside_the_startup_window_is_recorded() {
    let id = "d1r2-window-in";
    let a = begin_attempt(id);
    record_exit_code_for_attempt(id, a, Some(ACCESS_VIOLATION), Duration::from_secs(5));
    assert_eq!(get(id), Some(StartFailure::ChildExitStatus(0xC000_0005)));
}

#[test]
fn an_exit_after_the_startup_window_is_not_a_start_failure() {
    let id = "d1r2-window-out";
    let a = begin_attempt(id);
    record_exit_code_for_attempt(id, a, Some(ACCESS_VIOLATION), Duration::from_secs(31));
    assert_eq!(get(id), None);
}

#[test]
fn an_older_childs_exit_does_not_overwrite_a_newer_attempts_spawn_error() {
    let id = "d1r2-stale-exit";
    let old = begin_attempt(id);
    let _new = begin_attempt(id);
    record_spawn_error(id, &io::Error::from_raw_os_error(4551));
    record_exit_code_for_attempt(id, old, Some(-1073741515), Duration::from_secs(1));
    assert_eq!(get(id), Some(StartFailure::SpawnOsError(4551)));
}

#[test]
fn an_older_childs_exit_records_nothing_against_a_new_attempt() {
    let id = "d1r2-stale-none";
    let old = begin_attempt(id);
    let new = begin_attempt(id);
    record_exit_code_for_attempt(id, old, Some(-1073741515), Duration::from_secs(1));
    assert_eq!(get(id), None);
    record_exit_code_for_attempt(id, new, Some(-1073741515), Duration::from_secs(1));
    assert_eq!(get(id), Some(StartFailure::ChildExitStatus(0xC000_0135)));
}

#[test]
fn an_older_child_seen_running_does_not_clear_a_newer_record() {
    let id = "d1r2-stale-clear";
    let old = begin_attempt(id);
    let new = begin_attempt(id);
    record_spawn_error(id, &io::Error::from_raw_os_error(4551));
    clear_if_current(id, old);
    assert_eq!(get(id), Some(StartFailure::SpawnOsError(4551)));
    clear_if_current(id, new);
    assert_eq!(get(id), None);
}

#[test]
fn stop_integration_clears_the_record() {
    let id = "d1r2-stop";
    record_exit_code(id, Some(-1073741515));
    let mut sup = crate::supervisor::Supervisor::new(std::env::temp_dir().join("d1r2-stop-logs"));
    sup.stop_integration(id).unwrap();
    assert_eq!(get(id), None);
}

#[test]
fn dropping_a_process_does_not_observe_it() {
    let src = include_str!("process.rs");
    let at = src
        .find("impl Drop for ManagedProcess")
        .expect("Drop impl present");
    let rest = &src[at..];
    let body = &rest[..rest.find("\n}\n").expect("end of Drop impl")];
    assert!(
        !body.contains("is_running()"),
        "Drop must not go through the recording is_running"
    );
}
