//! B23 defect 2, the invariant that RC build 35800402767 broke.
//!
//! Scrubbing a partner's stdout at write time with the FULL rule set rewrote
//! the working directory a child reported, and `bug9_working_dir_tests` reads
//! that path back out of the log and canonicalizes it. `canonicalize` requires
//! the path to EXIST, so a substituted component cannot survive it — the test
//! failed with "os error 123, The filename, directory name, or volume label
//! syntax is incorrect" on `C:\Users\<user>\...`. The test was right; the
//! scrubbing was in the wrong layer.
//!
//! The pre-existing Windows test covers Windows. This is its Linux analogue,
//! and it is the guard that can actually run on a developer's machine and in
//! the Linux half of CI. The path it builds is deliberately made of components
//! the FULL rule set WOULD rewrite — a 16+ character lowercase hex run
//! (`redact_serial`) and a dotted quad (`redact_ipv4`) — so the test is
//! meaningful rather than incidentally passing.

use std::path::Path;

use super::ManagedProcess;

#[cfg(not(windows))]
#[test]
fn a_partner_path_survives_the_scrubbing_pipe_end_to_end() {
    let unique = format!(
        "fem-redact-probe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let root = std::env::temp_dir().join(unique);
    // Both components are rewritten by `scrub_line` and must NOT be by
    // `scrub_partner_line`. If this test ever passes with a path that the full
    // scrubber would leave alone, it has stopped proving anything.
    let cwd = root.join("deadbeefdeadbeef12").join("192.168.1.100");
    std::fs::create_dir_all(&cwd).expect("test cwd");
    let log_dir = root.join("logs");

    let mut proc = ManagedProcess::spawn_in(
        "redact-probe",
        "sh",
        &["-c", "pwd"],
        &log_dir,
        Some(cwd.as_path()),
    )
    .expect("spawn should succeed");
    let _ = proc.child.wait();
    // The child is gone, so both pipes are at EOF; this only waits for the
    // scrubber threads to flush what it already wrote.
    proc.drain_logs();

    let logged = std::fs::read_to_string(log_dir.join("redact-probe_stdout.log"))
        .expect("stdout log must exist");
    let reported = logged.trim();

    let actual = Path::new(reported).canonicalize().unwrap_or_else(|e| {
        panic!(
            "child reported {reported:?}, not a real path: {e}. Write-time \
             scrubbing has started rewriting partner paths again — that is the \
             exact failure that broke bug9_working_dir_tests in CI."
        )
    });
    assert_eq!(
        actual,
        cwd.canonicalize().expect("canonicalize target"),
        "the working directory a partner reports must reach the log intact"
    );

    let _ = std::fs::remove_dir_all(&root);
}
