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
