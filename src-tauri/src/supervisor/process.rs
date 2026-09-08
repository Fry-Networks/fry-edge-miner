use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::Duration;

use chrono::{DateTime, Utc};
use tracing::{info, warn};

/// Upper bound on how long `CreateProcess` may hold the spawning thread.
///
/// WP6 (2026-09-07, v0.4.24): toggling an integration whose staged binary was
/// corrupted left the card on INSTALLING for 58–283 s (until a human
/// dismissed Windows' modal "Unsupported 16-Bit Application" box) or >350 s
/// (Defender scan-on-execute, no dialog), although `toggle_integration`
/// wraps `start()` in a 60 s `tokio::time::timeout`. The spawn is
/// synchronous, so the task that timeout would cancel is parked inside
/// `CreateProcess` and can never be preempted. Keep this well under
/// `TOGGLE_STEP_TIMEOUT` so the toggle's own bound stays meaningful.
pub const SPAWN_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(windows)]
mod loader_error_mode {
    #[link(name = "kernel32")]
    extern "system" {
        fn SetThreadErrorMode(new_mode: u32, old_mode: *mut u32) -> i32;
    }
    const SEM_FAILCRITICALERRORS: u32 = 0x0001;
    const SEM_NOOPENFILEERRORBOX: u32 = 0x8000;

    /// RAII guard: while alive, the Windows loader reports a bad image to the
    /// caller as an error (`ERROR_BAD_EXE_FORMAT`) instead of raising a modal
    /// hard-error box on this thread. Restores the previous mode on drop.
    pub struct Suppressed(u32);

    pub fn suppress() -> Suppressed {
        let mut old = 0u32;
        // SAFETY: plain Win32 call with a valid out-pointer; affects only this thread.
        unsafe {
            SetThreadErrorMode(SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX, &mut old);
        }
        Suppressed(old)
    }

    impl Drop for Suppressed {
        fn drop(&mut self) {
            // SAFETY: restoring the mode this thread had before `suppress`.
            unsafe {
                SetThreadErrorMode(self.0, std::ptr::null_mut());
            }
        }
    }
}

#[cfg(not(windows))]
mod loader_error_mode {
    pub struct Suppressed;
    pub fn suppress() -> Suppressed {
        Suppressed
    }
}

/// Run `create` (the actual `Command::spawn`) on its own thread with the
/// loader's modal error boxes suppressed, and wait at most `bound` for it.
/// A creation that completes after the caller gave up is killed and logged —
/// never adopted as an untracked child.
///
/// The hand-off is a rendezvous (`sync_channel(0)`): `send` only succeeds
/// while the caller is actually receiving. With a buffered channel a child
/// created in the instant between the caller's timeout and the receiver
/// being dropped would be delivered into a buffer nobody reads, and the
/// process would keep running untracked (`Child` does not kill on drop).
fn spawn_bounded<F>(integration_id: &str, bound: Duration, create: F) -> io::Result<Child>
where
    F: FnOnce() -> io::Result<Child> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::sync_channel::<io::Result<Child>>(0);
    let id = integration_id.to_string();
    std::thread::Builder::new()
        .name(format!("spawn-{id}"))
        .spawn(move || {
            let _quiet = loader_error_mode::suppress();
            let result = create();
            if let Err(std::sync::mpsc::SendError(late)) = tx.send(result) {
                if let Ok(mut child) = late {
                    warn!(
                        integration = %id,
                        pid = child.id(),
                        "Process creation completed after the caller timed out — killing the untracked child"
                    );
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        })?;
    match rx.recv_timeout(bound) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "process creation for {integration_id} did not complete within {}s \
                 (Windows loader dialog or on-access scan holding CreateProcess)",
                bound.as_secs()
            ),
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(io::Error::new(
            io::ErrorKind::Other,
            format!("process creation thread for {integration_id} ended without a result"),
        )),
    }
}

/// A managed child process with metadata
pub struct ManagedProcess {
    child: Child,
    pub integration_id: String,
    #[allow(dead_code)] // Phase 3: process metadata
    pub started_at: DateTime<Utc>,
    #[allow(dead_code)] // Phase 3: process metadata
    pub log_dir: PathBuf,
}

impl ManagedProcess {
    /// Spawn a new child process with stdout/stderr redirected to log files
    pub fn spawn(
        integration_id: &str,
        command: &str,
        args: &[&str],
        log_dir: &Path,
    ) -> io::Result<Self> {
        std::fs::create_dir_all(log_dir)?;
        let stdout_path = log_dir.join(format!("{}_stdout.log", integration_id));
        let stderr_path = log_dir.join(format!("{}_stderr.log", integration_id));
        let stdout_file = std::fs::File::create(&stdout_path)?;
        let stderr_file = std::fs::File::create(&stderr_path)?;

        info!(
            integration = integration_id,
            command = command,
            "Spawning process"
        );

        let command_owned = command.to_string();
        let args_owned: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let child = spawn_bounded(integration_id, SPAWN_TIMEOUT, move || {
            super::platform::command(&command_owned)
                .args(&args_owned)
                .stdout(Stdio::from(stdout_file))
                .stderr(Stdio::from(stderr_file))
                .spawn()
        })?;

        Ok(Self {
            child,
            integration_id: integration_id.to_string(),
            started_at: Utc::now(),
            log_dir: log_dir.to_path_buf(),
        })
    }

    /// Check if the process is still running (non-blocking)
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Get the process ID
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Stop the process. Sends kill signal, waits up to timeout.
    pub fn stop(&mut self, timeout: Duration) -> io::Result<()> {
        info!(
            integration = self.integration_id,
            pid = self.child.id(),
            "Stopping process"
        );

        self.child.kill()?;

        let start = std::time::Instant::now();
        loop {
            match self.child.try_wait()? {
                Some(_status) => return Ok(()),
                None if start.elapsed() >= timeout => {
                    warn!(
                        integration = self.integration_id,
                        "Process did not exit within timeout"
                    );
                    return Ok(());
                }
                None => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        if self.is_running() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod wp6_spawn_tests {
    //! WP6 live finding (2026-09-07, FEM v0.4.24): toggling an integration
    //! whose staged binary is corrupted left the card on INSTALLING for
    //! 58–283 s (until a human dismissed Windows' modal "Unsupported 16-Bit
    //! Application" box) or >350 s (Defender scan-on-execute, no dialog),
    //! although `toggle_integration` wraps `start()` in a 60 s
    //! `tokio::time::timeout`. The spawn is synchronous, so the task the
    //! timeout would cancel is parked inside `CreateProcess` and can never be
    //! preempted. These tests pin the behaviour of the choke point the
    //! supervisor-managed integrations (Fry dVPN, Iagon, Mysterium, Titan) go
    //! through: `ManagedProcess::spawn`. SpaceAcres and Olostep call
    //! `Command::spawn` directly from their own `start()` and are NOT covered
    //! by this bound.
    use super::*;

    #[cfg(windows)]
    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "fem-wp6-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|x| x.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    /// A 200-byte "MZ" stub is exactly WP6 trial 1/4 (a real farmer/SDK
    /// binary truncated after its DOS header). Windows refuses to run it —
    /// but pre-fix it refused *interactively*, blocking the spawning thread
    /// behind a modal loader dialog. Post-fix the spawn must come back with an
    /// `Err` on its own, promptly, with no dialog.
    /// Test-only: the hard-error dialog is governed by the PROCESS error mode,
    /// which children inherit. A test binary launched from a tool host that
    /// already suppresses dialogs (Node.js does) can never show the box, so the
    /// test first resets the process to Windows' default (dialogs enabled) —
    /// the context FEM actually runs in when started from Explorer/NSIS.
    #[cfg(windows)]
    mod error_mode {
        #[link(name = "kernel32")]
        extern "system" {
            pub fn GetErrorMode() -> u32;
            pub fn SetErrorMode(mode: u32) -> u32;
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_corrupted_mz_stub_fails_to_spawn_promptly_without_a_loader_dialog() {
        let inherited = unsafe { error_mode::GetErrorMode() };
        let previous = unsafe { error_mode::SetErrorMode(0) };
        eprintln!("process error mode inherited=0x{inherited:04x} (reset to 0 for the test, was 0x{previous:04x})");
        struct RestoreMode(u32);
        impl Drop for RestoreMode {
            fn drop(&mut self) {
                unsafe {
                    error_mode::SetErrorMode(self.0);
                }
            }
        }
        let _restore = RestoreMode(previous);
        let dir = temp_dir("stub");
        let exe = dir.join("sdk_client.exe");
        // The first 200 bytes of a REAL PE (this very test binary): a complete
        // DOS header whose e_lfanew points past EOF — byte-for-byte the shape
        // of the live trials' truncated sdk_client.exe / space-acres.exe. A
        // synthetic "MZ"+zeros stub is refused instantly with no dialog and
        // does NOT reproduce the bug (verified 2026-09-07).
        let me = std::env::current_exe().expect("current exe");
        let real_head: Vec<u8> = std::fs::read(&me).expect("read self").into_iter().take(200).collect();
        assert_eq!(real_head.len(), 200);
        assert_eq!(&real_head[..2], b"MZ");
        std::fs::write(&exe, &real_head).expect("write stub");
        let log_dir = dir.join("logs");

        let (tx, rx) = std::sync::mpsc::channel();
        let exe_s = exe.to_string_lossy().to_string();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let r = ManagedProcess::spawn("wp6-stub", &exe_s, &[], &log_dir).map(|_| ());
            let _ = tx.send((r, started.elapsed()));
        });
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok((Err(e), elapsed)) => {
                assert!(
                    elapsed < Duration::from_secs(10),
                    "spawn errored but only after {elapsed:?}"
                );
                eprintln!("spawn refused promptly in {elapsed:?}: {e}");
            }
            Ok((Ok(()), elapsed)) => panic!("a 200-byte MZ stub must not spawn successfully (took {elapsed:?})"),
            Err(_) => panic!(
                "BUG 2+3 (WP6) REPRODUCED: ManagedProcess::spawn of a corrupted MZ stub has been blocked for >10 s \
                 (modal loader dialog or on-access scan holding CreateProcess) — the 60 s toggle timeout can never fire"
            ),
        }
    }

    /// WP6 trial 3 shape: `CreateProcess` itself stalls (on-access scan) with
    /// no dialog at all. The caller must get a `TimedOut` error at the bound
    /// instead of waiting on the OS.
    #[test]
    fn a_process_creation_that_never_returns_is_bounded() {
        let started = std::time::Instant::now();
        let r = spawn_bounded("wp6-stall", Duration::from_millis(300), || {
            std::thread::sleep(Duration::from_secs(3));
            Err(io::Error::new(io::ErrorKind::Other, "creation finished far too late"))
        });
        let elapsed = started.elapsed();
        assert!(matches!(&r, Err(e) if e.kind() == io::ErrorKind::TimedOut), "{r:?}");
        assert!(elapsed < Duration::from_secs(2), "spawn_bounded returned only after {elapsed:?}");
    }

    /// Test-only: liveness of the late child is judged on a SYNCHRONIZE handle
    /// opened while the child is certainly alive. Windows cannot recycle a PID
    /// while a handle to the process object is open, so the probe can never
    /// mistake an unrelated newcomer for the late child (a `tasklist`-by-PID
    /// probe did exactly that on 2026-09-07: OpenConsole.exe reused the killed
    /// child's PID within 2 s on a loaded box).
    #[cfg(windows)]
    mod late_child_handle {
        #[link(name = "kernel32")]
        extern "system" {
            pub fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> isize;
            pub fn WaitForSingleObject(handle: isize, milliseconds: u32) -> u32;
            pub fn CloseHandle(handle: isize) -> i32;
        }
        pub const SYNCHRONIZE: u32 = 0x0010_0000;
        pub const WAIT_OBJECT_0: u32 = 0;
    }

    /// A child that comes into existence after the caller gave up must be
    /// killed by the spawn thread, never left running untracked.
    #[cfg(windows)]
    #[test]
    fn a_child_created_after_the_caller_gave_up_is_killed_not_adopted() {
        let late = std::sync::Arc::new(std::sync::Mutex::new(None::<(u32, isize)>));
        let slot = late.clone();
        let r = spawn_bounded("wp6-late", Duration::from_millis(200), move || {
            std::thread::sleep(Duration::from_millis(700));
            let child = super::super::platform::command("cmd.exe")
                .args(["/c", "ping -n 60 127.0.0.1 >nul"])
                .spawn()?;
            // Pin the PID before handing the child back: from here on the
            // process object outlives the kill until the test closes it.
            let handle = unsafe { late_child_handle::OpenProcess(late_child_handle::SYNCHRONIZE, 0, child.id()) };
            *slot.lock().unwrap() = Some((child.id(), handle));
            Ok(child)
        });
        assert!(matches!(&r, Err(e) if e.kind() == io::ErrorKind::TimedOut), "{r:?}");
        let (pid, handle) = {
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            loop {
                if let Some(v) = *late.lock().unwrap() {
                    break v;
                }
                assert!(std::time::Instant::now() < deadline, "the late child was never created");
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        assert_ne!(handle, 0, "OpenProcess failed for the late child pid {pid}");
        // 2 s bound, waited on the process object itself (the kill lands at
        // creation time, so this is observed within milliseconds).
        let wait = unsafe { late_child_handle::WaitForSingleObject(handle, 2_000) };
        let diagnostics = if wait == late_child_handle::WAIT_OBJECT_0 {
            String::new()
        } else {
            let listing = std::process::Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}"), "/NH"])
                .output()
                .expect("tasklist");
            String::from_utf8_lossy(&listing.stdout).to_string()
        };
        unsafe { late_child_handle::CloseHandle(handle) };
        assert_eq!(
            wait,
            late_child_handle::WAIT_OBJECT_0,
            "late child pid {pid} is still alive 2 s after the caller timed out (wait=0x{wait:x}):\n{diagnostics}"
        );
    }

    /// Race the deadline from both sides: creations landing just before and
    /// just after `bound`. Every child the closure created must end up either
    /// returned to the caller (who then owns and kills it) or killed by the
    /// spawn thread — never left running. Guards the rendezvous hand-off in
    /// `spawn_bounded`: with a buffered channel a creation that lands in the
    /// instant between the caller's timeout and the receiver drop is
    /// delivered to nobody and the process survives untracked.
    #[cfg(windows)]
    #[test]
    fn every_child_created_around_the_deadline_is_either_returned_or_killed() {
        let mut leaked = Vec::new();
        let mut returned = 0usize;
        let mut timed_out = 0usize;
        for i in 0..12u64 {
            let slot = std::sync::Arc::new(std::sync::Mutex::new(None::<(u32, isize)>));
            let slot_w = slot.clone();
            let delay = Duration::from_millis(30 + (i % 5) * 5); // 30..50 ms around a 40 ms bound
            let r = spawn_bounded("wp6-race", Duration::from_millis(40), move || {
                std::thread::sleep(delay);
                let child = super::super::platform::command("cmd.exe")
                    .args(["/c", "ping -n 60 127.0.0.1 >nul"])
                    .spawn()?;
                let handle = unsafe { late_child_handle::OpenProcess(late_child_handle::SYNCHRONIZE, 0, child.id()) };
                *slot_w.lock().unwrap() = Some((child.id(), handle));
                Ok(child)
            });
            match r {
                Ok(mut child) => {
                    returned += 1;
                    let _ = child.kill();
                    let _ = child.wait();
                }
                Err(e) if e.kind() == io::ErrorKind::TimedOut => timed_out += 1,
                Err(e) => panic!("unexpected spawn error: {e}"),
            }
            let (pid, handle) = {
                let deadline = std::time::Instant::now() + Duration::from_secs(3);
                loop {
                    if let Some(v) = *slot.lock().unwrap() {
                        break v;
                    }
                    assert!(std::time::Instant::now() < deadline, "attempt {i}: the child was never created");
                    std::thread::sleep(Duration::from_millis(5));
                }
            };
            assert_ne!(handle, 0, "attempt {i}: OpenProcess failed for pid {pid}");
            let wait = unsafe { late_child_handle::WaitForSingleObject(handle, 2_000) };
            unsafe { late_child_handle::CloseHandle(handle) };
            if wait != late_child_handle::WAIT_OBJECT_0 {
                leaked.push(pid);
            }
        }
        eprintln!("deadline race: {returned} returned to the caller, {timed_out} timed out, {} leaked", leaked.len());
        assert!(leaked.is_empty(), "children left running after spawn_bounded: {leaked:?}");
    }
}
