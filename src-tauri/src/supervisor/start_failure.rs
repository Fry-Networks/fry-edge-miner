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

/// Why the most recent START attempt failed: the spawn's raw OS error (e.g.
/// 4551, App Control) or the child's startup exit status (NTSTATUS, e.g.
/// 0xC0000135). Recorded per integration; read by `titan::classify_not_running`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartFailure {
    SpawnOsError(i32),
    ChildExitStatus(u32),
}

/// Attempt number of the newest start attempt per integration. A record is only
/// written or cleared on behalf of the process that belongs to that attempt.
fn attempts() -> &'static Mutex<HashMap<String, u64>> {
    static ATTEMPTS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    ATTEMPTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn is_current(id: &str, attempt: u64) -> bool {
    attempts()
        .lock()
        .ok()
        .is_some_and(|map| map.get(id) == Some(&attempt))
}

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

/// Start a new attempt for `id`: it supersedes every earlier attempt (whose
/// children can no longer record or clear anything) and drops the old record.
pub(crate) fn begin_attempt(id: &str) -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let attempt = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut map) = attempts().lock() {
        map.insert(id.to_string(), attempt);
    }
    clear(id);
    attempt
}

/// Record a child's exit status, but only if it belongs to the current attempt
/// and died within `STARTUP_WINDOW` of being spawned: a child that ran for a
/// while and then crashed is not a start failure (the log tail explains it).
pub(crate) fn record_exit_code_for_attempt(
    id: &str,
    attempt: u64,
    code: Option<i32>,
    alive_for: std::time::Duration,
) {
    if alive_for < STARTUP_WINDOW && is_current(id, attempt) {
        record_exit_code(id, code);
    }
}

/// Clear the record because the child of `attempt` was observed running, unless
/// a newer attempt has since taken over.
pub(crate) fn clear_if_current(id: &str, attempt: u64) {
    if is_current(id, attempt) {
        clear(id);
    }
}

#[cfg(test)]
#[path = "start_failure_record_tests.rs"]
mod start_failure_record_tests;

#[cfg(test)]
#[path = "start_failure_r2_tests.rs"]
mod start_failure_r2_tests;
