//! D1: the start-failure record. The store is process-global and tests run in
//! parallel, so every test uses its own integration id.

use super::*;

#[test]
fn ntstatus_is_recovered_from_a_negative_exit_code() {
    assert_eq!(ntstatus_from_exit_code(-1073741515), Some(0xC000_0135));
    assert_eq!(ntstatus_from_exit_code(-1073741502), Some(0xC000_0142));
}

#[test]
fn ordinary_exit_codes_are_not_ntstatus() {
    for code in [0, 1, 2, 255, 4551] {
        assert_eq!(ntstatus_from_exit_code(code), None, "code {code}");
    }
}

#[test]
fn a_spawn_error_is_recorded_with_its_raw_code() {
    let id = "d1-rec-spawn";
    record_spawn_error(id, &io::Error::from_raw_os_error(4551));
    assert_eq!(get(id), Some(StartFailure::SpawnOsError(4551)));
}

#[test]
fn a_spawn_error_without_a_raw_code_records_nothing() {
    let id = "d1-rec-nocode";
    record_spawn_error(id, &io::Error::other("no code"));
    assert_eq!(get(id), None);
}

#[test]
fn an_exit_status_is_recorded() {
    let id = "d1-rec-exit";
    record_exit_code(id, Some(-1073741515));
    assert_eq!(get(id), Some(StartFailure::ChildExitStatus(0xC000_0135)));
}

#[test]
fn an_ordinary_exit_or_a_missing_code_records_nothing() {
    let id = "d1-rec-plain";
    record_exit_code(id, Some(1));
    record_exit_code(id, None);
    assert_eq!(get(id), None);
}

#[test]
fn clearing_forgets_the_record() {
    let id = "d1-rec-clear";
    record_exit_code(id, Some(-1073741515));
    clear(id);
    assert_eq!(get(id), None);
}

#[test]
fn the_latest_attempt_wins() {
    let id = "d1-rec-latest";
    record_spawn_error(id, &io::Error::from_raw_os_error(4551));
    record_exit_code(id, Some(-1073741502));
    assert_eq!(get(id), Some(StartFailure::ChildExitStatus(0xC000_0142)));
    clear(id);
    record_spawn_error(id, &io::Error::from_raw_os_error(4551));
    assert_eq!(get(id), Some(StartFailure::SpawnOsError(4551)));
}

#[test]
fn records_are_per_integration() {
    record_exit_code("d1-rec-a", Some(-1073741515));
    assert_eq!(get("d1-rec-b"), None);
}
