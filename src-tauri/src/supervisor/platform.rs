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

/// Run a command to completion with a hard deadline.
///
/// `Command::output()` blocks forever if the child never exits — a dead Docker
/// daemon leaves `docker compose ps` hanging on its named pipe, and because
/// health checks run inside the PoC reporting tick, one such call froze the
/// whole app for hours (v0.4.8 field incident). This spawns, polls, and kills
/// the child at the deadline instead, returning a TimedOut error.
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
                    let _ = child.wait();
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
            started.elapsed() < Duration::from_secs(30),
            "returned after {:?} — deadline not enforced",
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
