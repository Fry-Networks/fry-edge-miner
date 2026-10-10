//! RED (precursor to a fix): a raw Windows code from a failed child start
//! reaches the user-facing error string unchanged.
//!
//! Path under test: every `ManagedProcess::spawn_full` partner start funnels
//! its `CreateProcess` through `spawn_bounded` (child module of `process`, so
//! it can reach the private fn). Its `io::Error` is surfaced verbatim by the
//! integrations' `start()` and then rendered by main.rs's startup pass as
//! `HealthStatus::Unhealthy(format!("Start failed: {}", e))`. So the text
//! below is exactly what the card shows.
//!
//! `io::Error::from_raw_os_error(4551)` is what std produces for
//! ERROR_CODE_INTEGRITY_BLOCK-style App Control refusals; on Windows its
//! Display ends in "(os error 4551)". Off-Windows std still appends
//! "(os error N)", so the leak is observable on this host too.
//!
//! Seam gap: no pure "child failure -> user text" function exists, and no
//! code path reads an `ExitStatus` of a partner at all (`try_wait` is only
//! matched against `Ok(None)`), so the NTSTATUS exit-code cases are injected
//! as raw OS errors carrying those values.

use super::spawn_bounded;
use std::io;
use std::time::Duration;

/// The text the user sees for a start that failed with `raw`.
fn user_facing(raw: i32) -> String {
    let err = spawn_bounded("dd-red", Duration::from_secs(5), move || {
        Err(io::Error::from_raw_os_error(raw))
    })
    .expect_err("an injected failure must come back as Err");
    format!("Start failed: {err}")
}

fn assert_human_readable(msg: &str, needles: &[&str]) {
    for n in needles {
        assert!(
            !msg.contains(n),
            "RAW CODE LEAKED to the user: {n:?} is present in {msg:?}"
        );
    }
}

#[test]
fn app_control_block_4551_does_not_leak_the_raw_code() {
    let msg = user_facing(4551);
    assert_human_readable(&msg, &["4551"]);
}

#[test]
fn dll_init_failed_0xc0000142_does_not_leak_the_raw_code() {
    let msg = user_facing(0xC000_0142u32 as i32);
    assert_human_readable(
        &msg,
        &["3221225794", "-1073741502", "0xC0000142", "c0000142"],
    );
}

#[test]
fn commitment_limit_0xc000012d_does_not_leak_the_raw_code() {
    let msg = user_facing(0xC000_012Du32 as i32);
    assert_human_readable(
        &msg,
        &["3221225773", "-1073741523", "0xC000012D", "c000012d"],
    );
}
