//! B4 defect 1 + B20 defect 1 (D-04: one lever, not two).
//!
//! Rust `Drop` was the only partner-termination path FEM had. Drop cannot run
//! under `TerminateProcess` — Task Manager's "End task", a crash, the updater
//! killing FEM — and Tauri v2's exit path calls `std::process::exit`, where
//! managed-state Drop is not guaranteed either. So every abnormal FEM death
//! left the partners it had spawned running, which is what leaves frynode.exe
//! holding :8088 and makes the NEXT launch look like a port conflict.
//!
//! A job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is the only Windows
//! mechanism that covers that case, because the kernel — not FEM — does the
//! killing when the last handle to the job goes away.

/// Strip line comments so the prose explaining a rule can never be what
/// satisfies the assertion about it.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// D-03: ALL partners join the job, including space-acres and OlostepBrowser.
/// Docker Desktop must NOT — it has to outlive FEM (B19).
#[test]
fn every_partner_spawn_site_joins_the_job_and_docker_desktop_does_not() {
    let adopt = "adopt_into_partner_job(";
    for (name, src, expected) in [
        ("process.rs", include_str!("process.rs"), 1usize),
        ("aem.rs", include_str!("../integrations/aem.rs"), 1),
        (
            "space_acres.rs",
            include_str!("../integrations/space_acres.rs"),
            1,
        ),
        (
            "docker_manager.rs",
            include_str!("../integrations/docker_manager.rs"),
            0,
        ),
    ] {
        let found = code_only(src).matches(adopt).count();
        assert_eq!(
            found, expected,
            "{name}: expected {expected} partner-job adoption(s), found {found}. \
             Docker Desktop must outlive FEM; every FEM-spawned partner must not."
        );
    }
}

/// B20 Done-when, delivered through B4's job rather than a second mechanism
/// (D-04): a per-child `SetPriorityClass` could not reach a partner's own
/// children, and Olostep's CPU is in its Chromium renderer/GPU/utility tree.
#[test]
fn the_partner_job_is_kill_on_close_and_below_normal() {
    let code = code_only(include_str!("platform.rs"));
    for flag in [
        "JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE",
        "JOB_OBJECT_LIMIT_PRIORITY_CLASS",
        "BELOW_NORMAL_PRIORITY_CLASS",
    ] {
        assert!(
            code.contains(flag),
            "the partner job must carry {flag} — without it either the orphans \
             survive or the partners run at NORMAL priority"
        );
    }
}

/// D-03's required mitigation, and the reason it takes more than one
/// assertion.
///
/// My first version of this test asserted only that the exit handler contains
/// `shutdown()`. That was vacuous for the property it is named after:
/// `Supervisor::shutdown` iterates `Supervisor.processes`, which ONLY
/// `start_integration` populates, while OlostepBrowser (aem.rs) and SpaceAcres
/// (space_acres.rs) are spawned with a bare `Command::spawn()` and adopted
/// straight into the kill-on-close job. They are never in that map, so the
/// handler could contain `shutdown()` and still have the kernel
/// TerminateProcess a farmer mid-plot on every ordinary quit — a regression
/// against master, where both survived FEM's exit.
///
/// So the guard has to pin the SOURCE OF TRUTH, not the presence of a call.
#[test]
fn a_normal_quit_stops_partners_gracefully_before_the_job_kills_them() {
    let code = code_only(include_str!("../main.rs"));

    let at = code.find("RunEvent::ExitRequested").expect(
        "main() must handle the exit event, or a normal quit is indistinguishable \
         from FEM being killed",
    );
    let end = code[at..]
        .find("\n        })")
        .map(|e| at + e)
        .unwrap_or(code.len());
    let handler = &code[at..end];
    assert!(
        handler.len() < 1200,
        "the exit-handler slice has widened to {} bytes — bound it, or anything \
         later in main.rs can satisfy the assertions below",
        handler.len()
    );

    let graceful = format!("stop{}partners{}gracefully(", "_", "_");
    assert!(
        handler.contains(&graceful),
        "the exit handler must run the graceful stop D-03 requires:\n{handler}"
    );
    let shutdown = "shutdown()";
    let graceful_at = handler.find(&graceful).expect("graceful stop present");
    let shutdown_at = handler
        .find(shutdown)
        .expect("the supervisor backstop must still run");
    assert!(
        graceful_at < shutdown_at,
        "the graceful stop must come BEFORE the supervisor backstop:\n{handler}"
    );

    // And the graceful stop must select from the REGISTRY, which is the only
    // source that includes partners the supervisor never tracked.
    let helper_at = code
        .find(&format!("fn stop{}partners{}gracefully", "_", "_"))
        .expect("the helper must exist");
    let helper_end = code[helper_at..]
        .find("\nfn main(")
        .map(|e| helper_at + e)
        .unwrap_or(code.len());
    let helper = &code[helper_at..helper_end];
    assert!(
        helper.contains("state.registry"),
        "the graceful stop must enumerate the REGISTRY; Supervisor.processes \
         cannot see a partner spawned outside start_integration:\n{helper}"
    );
    // BL-1, found by the pre-tag review of 0.4.34. This assertion used to read
    // `integration.stop()`, which matched the bug rather than the intent. The
    // intent — stated in its own message — is "through the TRAIT", because that
    // is what reaches partners the supervisor never tracked. `stop_for_exit()` is
    // equally a trait method and reaches them identically. But WHICH trait method
    // turned out to matter: for Pawns the bare `stop()` means
    // `StopReason::UserDisable`, so it appended a §5.8 consent withdrawal on every
    // ordinary quit and reintroduced B18 once per launch. So the needle is
    // retargeted, NOT loosened — it now pins the exact method and additionally
    // forbids the one that caused the defect.
    let trait_stop = format!("integration.stop_for{}()", "_exit");
    assert!(
        helper.contains(&trait_stop),
        "it must stop each integration through the EXIT-SCOPED trait method, which \
         reaches the bare-spawn partners without recording a consent withdrawal:\n{helper}"
    );
    let bare_stop = format!("integration.stop{}", "()");
    assert!(
        !helper.contains(&bare_stop),
        "the bare stop() is StopReason::UserDisable for Pawns and writes a §5.8 \
         withdrawal on every quit — that is BL-1:\n{helper}"
    );
    assert!(
        !helper.contains("processes"),
        "it must not go back through Supervisor.processes — that is the map that \
         cannot see them:\n{helper}"
    );
}

/// The fact that makes the test above meaningful: these two really are outside
/// the supervisor's map, so the registry path is the ONLY one that reaches them.
/// If either ever starts going through `start_integration`, the reasoning above
/// changes and this should be revisited rather than silently still passing.
#[test]
fn the_two_bare_spawn_partners_are_not_supervisor_tracked() {
    let tracked = format!("start{}integration", "_");
    for (name, src) in [
        ("aem.rs", include_str!("../integrations/aem.rs")),
        (
            "space_acres.rs",
            include_str!("../integrations/space_acres.rs"),
        ),
    ] {
        let code = code_only(src);
        assert!(
            !code.contains(&tracked),
            "{name} now registers with the supervisor — D-03's mitigation \
             reasoning assumed it does not, so re-check the exit path"
        );
        assert!(
            code.contains(".spawn()"),
            "{name} is expected to spawn its partner directly"
        );
    }
}

/// The mechanism itself, on the only platform where it exists. Builds a
/// TEST-LOCAL job with the same flag so nothing touches FEM's own job.
#[cfg(windows)]
mod windows_behaviour {
    use std::os::windows::io::AsRawHandle;

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
        fn CloseHandle(handle: isize) -> i32;
    }

    #[repr(C)]
    #[derive(Default)]
    struct IoCounters {
        a: u64,
        b: u64,
        c: u64,
        d: u64,
        e: u64,
        f: u64,
    }

    #[repr(C)]
    #[derive(Default)]
    struct BasicLimit {
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
    struct ExtendedLimit {
        basic: BasicLimit,
        io: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    #[test]
    fn a_process_in_a_kill_on_close_job_dies_when_the_last_handle_closes() {
        const EXTENDED_LIMIT_CLASS: i32 = 9;
        const KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

        // SAFETY: plain Win32 calls with a correctly sized #[repr(C)] struct.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        assert_ne!(job, 0, "could not create a test job object");
        let info = ExtendedLimit {
            basic: BasicLimit {
                limit_flags: KILL_ON_JOB_CLOSE,
                ..Default::default()
            },
            ..Default::default()
        };
        let set = unsafe {
            SetInformationJobObject(
                job,
                EXTENDED_LIMIT_CLASS,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<ExtendedLimit>() as u32,
            )
        };
        assert_ne!(set, 0, "could not set kill-on-close on the test job");

        let mut child = crate::supervisor::platform::command("cmd")
            .args(["/c", "ping", "-n", "120", "127.0.0.1"])
            .spawn()
            .expect("could not spawn the test child");
        let assigned = unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as isize) };
        assert_ne!(assigned, 0, "could not assign the test child to the job");
        assert!(
            matches!(child.try_wait(), Ok(None)),
            "the test child must still be running before the job is closed"
        );

        // This is the whole point: FEM never closes its job handle, so the
        // close happens when FEM's process object is torn down — however FEM
        // died — and the kernel kills everything inside.
        unsafe { CloseHandle(job) };

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => break,
                Ok(None) if std::time::Instant::now() >= deadline => {
                    let _ = child.kill();
                    panic!("closing the last job handle did not kill the process within 5s");
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
                Err(e) => panic!("could not wait on the test child: {e}"),
            }
        }
    }
}

/// B20 defect 2. The job covers what FEM SPAWNS. It cannot cover the common
/// Olostep case, because FEM's own staged config (`"auto-start-enabled": true`)
/// makes Windows start Olostep at logon and FEM then ADOPTS it — `is_running()`
/// answers true from an image-name probe and `start()` returns before it ever
/// reaches a spawn. The reported 40% CPU is in the Chromium renderer/GPU/
/// utility children too, which FEM holds no handle to either. So the second
/// lever is by IMAGE, and it must stay unelevated: B20's Done-when is "no
/// popups, no elevation".
#[test]
fn the_reprioritise_script_covers_every_process_of_the_image_without_elevation() {
    let script = super::below_normal_script("OlostepBrowser");
    assert!(
        script.contains("Get-Process OlostepBrowser"),
        "the script must target the image, not a handle FEM holds: {script}"
    );
    assert!(
        script.contains("'BelowNormal'"),
        "the script must set BelowNormal: {script}"
    );
    assert!(
        script.contains("-ne 'BelowNormal'"),
        "already-lowered processes must be skipped so the tick stays cheap: {script}"
    );
    assert!(
        !script.contains("-Verb RunAs"),
        "lowering priority on same-user processes needs no admin, and B20's \
         soak requires zero elevation: {script}"
    );
    assert!(
        !script.contains('\n'),
        "the script is folded into an existing one-line -Command: {script}"
    );
}

/// …and it has to be wired into the tick that already runs, not into a new
/// process spawned every 30 seconds.
#[test]
fn the_reprioritise_runs_on_the_existing_olostep_tick() {
    let code = code_only(include_str!("../integrations/aem.rs"));
    let call = "below_normal_script(";
    assert_eq!(
        code.matches(call).count(),
        1,
        "Olostep's priority must be lowered from exactly one place"
    );
    let at = code.find(call).expect("the call must exist");
    let sample_at = code
        .find("fn resource_sample")
        .expect("the per-tick sampler must still exist");
    let breach_at = code
        .find("fn resource_breach")
        .expect("the resource guard must still exist");
    assert!(
        sample_at < at && at < breach_at,
        "the reprioritise must be folded into resource_sample's existing \
         PowerShell, so the tick starts no extra process"
    );
}
