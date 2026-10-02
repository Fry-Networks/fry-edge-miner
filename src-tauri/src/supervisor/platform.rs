use std::io;
/// Platform-specific process management utilities.
/// For v1, std::process::Child::kill() is sufficient on both platforms.
/// Phase 3.5 will add graceful SIGTERM on Unix and proper
/// TerminateProcess/WM_CLOSE on Windows.
use std::process::Child;

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

/// B4/B20: the kill-on-close Job Object every FEM-spawned partner joins.
///
/// Rust `Drop` was the ONLY partner-termination path — `ManagedProcess::drop`
/// and `Supervisor::drop`. Drop cannot run under `TerminateProcess` (Task
/// Manager's "End task", a crash, the updater killing FEM), and Tauri v2's exit
/// path calls `std::process::exit`, where managed-state Drop is not guaranteed
/// either. So any abnormal FEM death orphaned every partner it had spawned —
/// which is what leaves frynode.exe holding :8088 and makes the next launch
/// look like a port conflict.
///
/// A job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is the only Windows
/// mechanism that survives that: the handle is deliberately never closed, so
/// the LAST handle to it goes away exactly when FEM's process object does,
/// however FEM died, and the kernel terminates the job. It also carries
/// `JOB_OBJECT_LIMIT_PRIORITY_CLASS` + `BELOW_NORMAL_PRIORITY_CLASS`, which is
/// B20's "partner processes run at BELOW_NORMAL" as ONE lever rather than a
/// second mechanism — and unlike a per-child `SetPriorityClass` it applies to
/// the partner's own children too, which is where Olostep's CPU actually goes.
#[cfg(windows)]
mod partner_job {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;

    use tracing::{info, warn};

    // Same raw-FFI idiom the crate already uses for SetThreadErrorMode
    // (supervisor/process.rs) — no new dependency.
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateJobObjectW(attributes: *const std::ffi::c_void, name: *const u16) -> isize;
        fn SetInformationJobObject(
            job: isize,
            info_class: i32,
            info: *const std::ffi::c_void,
            info_len: u32,
        ) -> i32;
        fn AssignProcessToJobObject(job: isize, process: isize) -> i32;
    }

    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: i32 = 9;
    const JOB_OBJECT_LIMIT_PRIORITY_CLASS: u32 = 0x0000_0020;
    const JOB_OBJECT_LIMIT_BREAKAWAY_OK: u32 = 0x0000_0800;
    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
    const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

    #[repr(C)]
    #[derive(Default)]
    struct IoCounters {
        read_operation_count: u64,
        write_operation_count: u64,
        other_operation_count: u64,
        read_transfer_count: u64,
        write_transfer_count: u64,
        other_transfer_count: u64,
    }

    #[repr(C)]
    #[derive(Default)]
    struct BasicLimitInformation {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct ExtendedLimitInformation {
        basic_limit_information: BasicLimitInformation,
        io_info: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    /// The limits FEM asks the kernel for. Pure, so the flag set is testable
    /// without creating a real job — B20's Done-when is "verified per process",
    /// and this is the single place the priority class is decided.
    pub(super) fn partner_limit_flags() -> u32 {
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_BREAKAWAY_OK
            | JOB_OBJECT_LIMIT_PRIORITY_CLASS
    }

    pub(super) fn partner_priority_class() -> u32 {
        BELOW_NORMAL_PRIORITY_CLASS
    }

    /// The job handle, created once and NEVER closed. Closing it is what kills
    /// the partners, and the only close that should ever happen is the
    /// process-teardown one.
    fn job() -> Option<isize> {
        static JOB: OnceLock<Option<isize>> = OnceLock::new();
        *JOB.get_or_init(|| {
            // SAFETY: plain Win32 calls. `CreateJobObjectW` with null
            // attributes and a null name creates an unnamed job owned by this
            // process; `SetInformationJobObject` is handed a correctly sized
            // `#[repr(C)]` struct.
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle == 0 {
                warn!("Could not create the partner job object — partners will not be killed on an abnormal FEM exit");
                return None;
            }
            let info = ExtendedLimitInformation {
                basic_limit_information: BasicLimitInformation {
                    limit_flags: partner_limit_flags(),
                    priority_class: partner_priority_class(),
                    ..Default::default()
                },
                ..Default::default()
            };
            let ok = unsafe {
                SetInformationJobObject(
                    handle,
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                    &info as *const _ as *const std::ffi::c_void,
                    std::mem::size_of::<ExtendedLimitInformation>() as u32,
                )
            };
            if ok == 0 {
                warn!("Could not set limits on the partner job object — continuing without them");
                return None;
            }
            info!(
                limit_flags = partner_limit_flags(),
                priority_class = partner_priority_class(),
                "Partner job object created (kill-on-close, below-normal priority)"
            );
            Some(handle)
        })
    }

    pub(super) fn adopt(child: &std::process::Child) {
        let Some(job) = job() else { return };
        // SAFETY: `as_raw_handle` yields a live process handle owned by `child`.
        let ok = unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as isize) };
        if ok == 0 {
            // Never fatal: a partner that could not join the job still runs,
            // it just is not covered by kill-on-close.
            warn!(
                pid = child.id(),
                "Could not assign a partner to the job object — it will not be killed on an abnormal FEM exit"
            );
        }
    }
}

/// B4: put a freshly spawned partner into FEM's kill-on-close job object, so no
/// abnormal FEM exit can leave it running, and B20: give it BELOW_NORMAL
/// priority through the same job. Best-effort — a failure must never stop an
/// integration starting.
pub fn adopt_into_partner_job(child: &std::process::Child) {
    #[cfg(windows)]
    partner_job::adopt(child);
    #[cfg(not(windows))]
    let _ = child;
}

/// B20: the PowerShell that lowers every process of `image` to BELOW_NORMAL.
///
/// A spawn-site priority cannot reach Olostep in the common case: FEM stages it
/// to autostart, so FEM usually ADOPTS an instance Windows started rather than
/// spawning one, and it cannot reach the Chromium renderer/GPU/utility children
/// where the CPU actually goes. Lowering priority on same-user processes needs
/// no elevation, which B20's Done-when ("no popups, no elevation") requires.
/// Pure so the script is testable with no disk and no Windows.
pub fn below_normal_script(image: &str) -> String {
    format!(
        "Get-Process {image} -ErrorAction SilentlyContinue | \
         Where-Object {{ $_.PriorityClass -ne 'BelowNormal' }} | \
         ForEach-Object {{ try {{ $_.PriorityClass = 'BelowNormal' }} catch {{}} }}"
    )
}

/// Default deadline for a short-lived CLI probe (`docker compose ps`,
/// `tasklist`, `netsh show`, PowerShell one-liners).
pub const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// B3: deadline for an elevation a HUMAN has to answer on the secure desktop.
///
/// `PROBE_TIMEOUT` is the generic short-lived-CLI-probe budget and was never a
/// human-answer budget, but `firewall::ensure_program_rules`,
/// `firewall::delete_rules` and `security_setup::run_hardening_elevated` all
/// wrapped their `Start-Process -Verb RunAs` in it. At 20s `output_bounded`
/// kills the requesting (unelevated) PowerShell while the consent dialog is
/// still on screen — the dialog itself belongs to the AppInfo service and
/// survives, so anyone who takes longer than 20s to answer can never grant the
/// rule. `titan.rs` already made exactly this argument for its own elevation
/// (`VC_REDIST_INSTALL_TIMEOUT`); this is the same treatment for the other two.
pub const UAC_ANSWER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

/// Deadline for commands that legitimately take a while (`docker compose up`,
/// image pulls, installers). Generous on purpose: a multi-layer pull over a
/// slow uplink can run many minutes, and a false timeout here fails a real
/// install — the point is only that it cannot hang forever.
pub const LONG_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(900);

/// B4/B20: the job object's own guarantees, and the spawn sites that rely on
/// them. Own file so the source-scan machinery stays out of the way of
/// platform.rs's behavioural tests.
#[cfg(test)]
#[path = "partner_job_tests.rs"]
mod partner_job_tests;

/// Poll `is_done` (analogous to `Child::try_wait().map(|o| o.is_some())`)
/// until it reports true, or `budget` elapses — whichever comes first.
/// NEVER blocks past `budget` even when `is_done` never becomes true.
///
/// Extracted so the deadline behavior itself is unit-testable without a real
/// (and, on Windows, essentially unmanufacturable) "process that ignores
/// `TerminateProcess`" — this is the one thing both the main wait loop and
/// the post-kill reap step below actually need to get right.
fn bounded_wait<F: FnMut() -> bool>(
    mut is_done: F,
    budget: std::time::Duration,
    poll: std::time::Duration,
) -> bool {
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
                    let reaped =
                        bounded_wait(|| matches!(child.try_wait(), Ok(Some(_))), REAP_GRACE, POLL);
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
    fn output_bounded(&mut self, timeout: std::time::Duration) -> io::Result<std::process::Output>;
}

impl BoundedOutput for std::process::Command {
    fn output_bounded(&mut self, timeout: std::time::Duration) -> io::Result<std::process::Output> {
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

/// B3 defect 5: the UAC-answer budget is enforced against the real source of
/// every elevation wrapper, so a future edit cannot quietly put one back on
/// the 20s probe budget. Own file so the source-scan machinery stays out of
/// the way of platform.rs's behavioural tests.
#[cfg(test)]
#[path = "uac_budget_tests.rs"]
mod uac_budget_tests;

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
        let done = bounded_wait(
            || false,
            Duration::from_millis(50),
            Duration::from_millis(5),
        );
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
