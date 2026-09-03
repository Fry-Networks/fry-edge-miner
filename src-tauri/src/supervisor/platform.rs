/// Platform-specific process management utilities.
/// For v1, std::process::Child::kill() is sufficient on both platforms.
/// Phase 3.5 will add graceful SIGTERM on Unix and proper
/// TerminateProcess/WM_CLOSE on Windows.
use std::process::Child;
use std::io;

/// Create a Command with CREATE_NO_WINDOW on Windows to suppress console popups.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Default deadline for a short-lived CLI probe (`docker compose ps`,
/// `tasklist`, `netsh show`, PowerShell one-liners).
pub const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Deadline for commands that legitimately take a while (`docker compose up`,
/// image pulls, installers). Generous on purpose: a multi-layer pull over a
/// slow uplink can run many minutes, and a false timeout here fails a real
/// install — the point is only that it cannot hang forever.
pub const LONG_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(900);

/// Poll `is_done` (analogous to `Child::try_wait().map(|o| o.is_some())`)
/// until it reports true, or `budget` elapses — whichever comes first.
/// NEVER blocks past `budget` even when `is_done` never becomes true.
///
/// Extracted so the deadline behavior itself is unit-testable without a real
/// (and, on Windows, essentially unmanufacturable) "process that ignores
/// `TerminateProcess`" — this is the one thing both the main wait loop and
/// the post-kill reap step below actually need to get right.
fn bounded_wait<F: FnMut() -> bool>(mut is_done: F, budget: std::time::Duration, poll: std::time::Duration) -> bool {
    let started = std::time::Instant::now();
    loop {
        if is_done() {
            return true;
        }
        if started.elapsed() >= budget {
            return false;
        }
        std::thread::sleep(poll);
    }
}

/// Grace period to let a just-killed child get reaped before we give up and
/// return anyway. `kill()` itself is fire-and-forget (it can fail silently —
/// already exited, permission denied, a wedged handle) and the OS does not
/// guarantee instant reaping even after a successful `TerminateProcess`, so
/// this must stay bounded too, not become a second unconditional `wait()`.
const REAP_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// Run a command to completion with a hard deadline.
///
/// `Command::output()` blocks forever if the child never exits — a dead Docker
/// daemon leaves `docker compose ps` hanging on its named pipe, and because
/// health checks run inside the PoC reporting tick, one such call froze the
/// whole app for hours (v0.4.8 field incident). This spawns, polls, and kills
/// the child at the deadline instead, returning a TimedOut error.
///
/// The post-kill reap is itself bounded (`REAP_GRACE`): the original
/// implementation called `child.wait()` unconditionally after `kill()`, which
/// is *also* an unbounded blocking call if `kill()` failed silently or the
/// child was slow to actually exit — turning a "bounded" probe into an
/// indefinite hang on whatever thread called it. If the child still hasn't
/// been reaped after the grace period, we log it and return TimedOut anyway,
/// accepting a leaked handle rather than hanging the caller — this function's
/// own contract ("cannot hang forever") was previously false for this branch.
///
/// NOTE: like `Command::output()`, this forces piped stdout/stderr — any
/// stdio the caller configured is overwritten.
pub fn output_bounded(
    cmd: &mut std::process::Command,
    timeout: std::time::Duration,
) -> io::Result<std::process::Output> {
    use std::process::Stdio;
    const POLL: std::time::Duration = std::time::Duration::from_millis(100);

    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait()? {
            Some(_) => return child.wait_with_output(),
            None => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let reaped = bounded_wait(
                        || matches!(child.try_wait(), Ok(Some(_))),
                        REAP_GRACE,
                        POLL,
                    );
                    if !reaped {
                        tracing::warn!(
                            timeout_s = timeout.as_secs(),
                            grace_s = REAP_GRACE.as_secs(),
                            "output_bounded: child did not reap within the grace period after kill() — \
                             leaking the handle rather than hanging the caller"
                        );
                    }
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("command timed out after {}s", timeout.as_secs()),
                    ));
                }
                std::thread::sleep(POLL);
            }
        }
    }
}

/// `.output()` with a deadline, as a drop-in method on `Command`.
pub trait BoundedOutput {
    fn output_bounded(
        &mut self,
        timeout: std::time::Duration,
    ) -> io::Result<std::process::Output>;
}

impl BoundedOutput for std::process::Command {
    fn output_bounded(
        &mut self,
        timeout: std::time::Duration,
    ) -> io::Result<std::process::Output> {
        output_bounded(self, timeout)
    }
}

/// Attempt to gracefully stop a child process.
/// Falls back to kill() for v1.
#[allow(dead_code)] // Phase 3: graceful process management
pub fn graceful_stop(child: &mut Child) -> io::Result<()> {
    child.kill()
}

/// Force-kill a child process.
#[allow(dead_code)] // Phase 3: graceful process management
pub fn force_kill(child: &mut Child) -> io::Result<()> {
    child.kill()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Bug 2 regression: `bounded_wait` must return by its budget even when
    /// the condition NEVER becomes true — this is the exact shape of the
    /// defect (`output_bounded`'s post-kill `child.wait()` was unconditional,
    /// i.e. equivalent to a budget of infinity). A real "process that
    /// survives TerminateProcess" cannot be manufactured portably in a unit
    /// test, so this targets the extracted, generic polling primitive
    /// directly with a closure that always returns `false`. Non-vacuity was
    /// confirmed via a mutation check (asserted the wrong way, observed
    /// FAILED, restored) — pasted in bug2.md, since bounded_wait is new code
    /// with no pre-fix version to diff against directly.
    #[test]
    fn bounded_wait_never_blocks_past_its_budget_even_when_the_condition_never_becomes_true() {
        let started = Instant::now();
        let done = bounded_wait(|| false, Duration::from_millis(50), Duration::from_millis(5));
        let elapsed = started.elapsed();
        assert!(!done, "condition never became true — must report not-done");
        assert!(
            elapsed < Duration::from_millis(300),
            "returned after {elapsed:?} — budget (50ms) not enforced"
        );
    }

    #[test]
    fn bounded_wait_returns_true_promptly_once_the_condition_flips() {
        let mut calls = 0u32;
        let started = Instant::now();
        let done = bounded_wait(
            || {
                calls += 1;
                calls >= 3
            },
            Duration::from_secs(5),
            Duration::from_millis(5),
        );
        assert!(done);
        assert!(
            started.elapsed() < Duration::from_millis(300),
            "should not wait anywhere near the 5s budget once the condition is true"
        );
    }

    /// Repro of the v0.4.8 freeze: a child that never exits must not block the
    /// caller forever. `Command::output()` would hang here indefinitely.
    #[test]
    fn hanging_child_is_killed_at_the_deadline() {
        let mut cmd = if cfg!(target_os = "windows") {
            let mut c = command("powershell");
            c.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 60"]);
            c
        } else {
            let mut c = command("sleep");
            c.arg("60");
            c
        };
        let started = Instant::now();
        let err = output_bounded(&mut cmd, Duration::from_secs(2)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut, "got {err:?}");
        assert!(
            started.elapsed() < Duration::from_secs(6),
            "returned after {:?} — deadline not enforced (2s cap)",
            started.elapsed()
        );
    }

    #[test]
    fn fast_child_returns_its_output() {
        let mut cmd = if cfg!(target_os = "windows") {
            let mut c = command("cmd");
            c.args(["/C", "echo bounded-ok"]);
            c
        } else {
            let mut c = command("echo");
            c.arg("bounded-ok");
            c
        };
        let out = output_bounded(&mut cmd, Duration::from_secs(20)).expect("should complete");
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("bounded-ok"));
    }

    #[test]
    fn missing_binary_still_errors_without_hanging() {
        let mut cmd = command("definitely-not-a-real-binary-xyz");
        let err = output_bounded(&mut cmd, Duration::from_secs(5)).unwrap_err();
        assert_ne!(err.kind(), std::io::ErrorKind::TimedOut);
    }
}
