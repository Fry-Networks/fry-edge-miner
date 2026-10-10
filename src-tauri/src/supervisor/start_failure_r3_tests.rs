//! D1 round 3: NTSTATUS recognition is exact, and a spawn error is attributed
//! to its own attempt only. Store is process-global; each test uses its own id.

use super::*;

#[test]
fn a_plain_minus_one_exit_is_not_an_ntstatus() {
    assert_eq!(ntstatus_from_exit_code(-1), None);
    assert_eq!(ntstatus_from_exit_code(0xFFFF_FFFFu32 as i32), None);
}

#[test]
fn customer_bit_codes_are_not_an_ntstatus() {
    assert_eq!(ntstatus_from_exit_code(0xE000_0001u32 as i32), None);
}

#[test]
fn ntstatus_error_codes_still_record() {
    assert_eq!(ntstatus_from_exit_code(-1073741515), Some(0xC000_0135));
    assert_eq!(
        ntstatus_from_exit_code(0xC0E9_0002u32 as i32),
        Some(0xC0E9_0002)
    );
    let id = "d1r3-nt-ok";
    let a = begin_attempt(id);
    record_exit_code_for_attempt(id, a, Some(-1073741515), std::time::Duration::from_secs(1));
    assert_eq!(get(id), Some(StartFailure::ChildExitStatus(0xC000_0135)));
}

#[test]
fn a_plain_minus_one_exit_records_nothing() {
    let id = "d1r3-minus-one";
    let a = begin_attempt(id);
    record_exit_code_for_attempt(id, a, Some(-1), std::time::Duration::from_secs(1));
    assert_eq!(get(id), None);
}

#[test]
fn a_spawn_error_for_the_current_attempt_is_recorded() {
    let id = "d1r3-spawn-cur";
    let a = begin_attempt(id);
    record_spawn_error_for_attempt(id, a, &io::Error::from_raw_os_error(4551));
    assert_eq!(get(id), Some(StartFailure::SpawnOsError(4551)));
}

#[test]
fn a_spawn_error_for_a_superseded_attempt_is_a_no_op() {
    let id = "d1r3-spawn-stale";
    let old = begin_attempt(id);
    let _new = begin_attempt(id);
    record_spawn_error_for_attempt(id, old, &io::Error::from_raw_os_error(4551));
    assert_eq!(get(id), None);
}

#[test]
fn a_late_old_spawn_error_does_not_overwrite_a_newer_record() {
    let id = "d1r3-spawn-late";
    let old = begin_attempt(id);
    let new = begin_attempt(id);
    record_spawn_error_for_attempt(id, new, &io::Error::from_raw_os_error(577));
    record_spawn_error_for_attempt(id, old, &io::Error::from_raw_os_error(4551));
    assert_eq!(get(id), Some(StartFailure::SpawnOsError(577)));
}
