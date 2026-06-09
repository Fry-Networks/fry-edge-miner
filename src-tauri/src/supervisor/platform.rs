/// Platform-specific process management utilities.
/// For v1, std::process::Child::kill() is sufficient on both platforms.
/// Phase 3.5 will add graceful SIGTERM on Unix and proper
/// TerminateProcess/WM_CLOSE on Windows.

use std::process::Child;
use std::io;

/// Attempt to gracefully stop a child process.
/// Falls back to kill() for v1.
pub fn graceful_stop(child: &mut Child) -> io::Result<()> {
    child.kill()
}

/// Force-kill a child process.
pub fn force_kill(child: &mut Child) -> io::Result<()> {
    child.kill()
}
