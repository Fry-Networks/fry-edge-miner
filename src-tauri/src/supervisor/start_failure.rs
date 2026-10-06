//! D1: the last START failure per integration, as a structured code.
//!
//! The Titan card used to guess the cause of a dead process (missing VC++
//! runtime) from the environment, and so blamed the runtime for an App Control
//! refusal. The supervisor is the only place that sees the raw spawn error and
//! the child's startup exit status, so it records them here and
//! `titan::classify_not_running` turns the record into card text.
//!
//! Latest attempt wins: every start attempt clears the record first. A child
//! observed running clears it too, so a stale code never explains a later,
//! unrelated failure. Plumbing only: nothing here changes restart behaviour.

use std::collections::HashMap;
use std::io;
use std::sync::{Mutex, OnceLock};

use crate::integrations::titan::StartFailure;

fn store() -> &'static Mutex<HashMap<String, StartFailure>> {
    static STORE: OnceLock<Mutex<HashMap<String, StartFailure>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn set(id: &str, failure: StartFailure) {
    if let Ok(mut map) = store().lock() {
        map.insert(id.to_string(), failure);
    }
}

/// The most recent recorded start failure for `id`, if any.
pub(crate) fn get(id: &str) -> Option<StartFailure> {
    store().lock().ok()?.get(id).copied()
}

/// Forget any recorded failure for `id`.
pub(crate) fn clear(id: &str) {
    if let Ok(mut map) = store().lock() {
        map.remove(id);
    }
}

/// Record a failed spawn, keeping the raw OS error code (if the error has one).
pub(crate) fn record_spawn_error(id: &str, e: &io::Error) {
    if let Some(code) = e.raw_os_error() {
        set(id, StartFailure::SpawnOsError(code));
    }
}

/// `Some(ntstatus)` when a child's exit code is an NTSTATUS failure code
/// (severity bits 0b11, e.g. -1073741515 == 0xC0000135). Ordinary small exit
/// codes (1, 2, ...) are the program's own and keep the log-tail diagnosis.
pub(crate) fn ntstatus_from_exit_code(code: i32) -> Option<u32> {
    let status = code as u32;
    (status & 0xC000_0000 == 0xC000_0000).then_some(status)
}

/// Record the exit status of a child that was observed dead.
pub(crate) fn record_exit_code(id: &str, code: Option<i32>) {
    if let Some(status) = code.and_then(ntstatus_from_exit_code) {
        set(id, StartFailure::ChildExitStatus(status));
    }
}

/// How long after spawn an exit still counts as a START failure.
pub(crate) const STARTUP_WINDOW: std::time::Duration = std::time::Duration::from_secs(30);

// RED stubs (round 2): no window, no attempt tracking. Replaced by the fix.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn begin_attempt(id: &str) -> u64 {
    clear(id);
    0
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn record_exit_code_for_attempt(
    id: &str,
    _attempt: u64,
    code: Option<i32>,
    _alive_for: std::time::Duration,
) {
    record_exit_code(id, code);
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn clear_if_current(id: &str, _attempt: u64) {
    clear(id);
}

#[cfg(test)]
#[path = "start_failure_record_tests.rs"]
mod start_failure_record_tests;

#[cfg(test)]
#[path = "start_failure_r2_tests.rs"]
mod start_failure_r2_tests;
