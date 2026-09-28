//! FAIL-6 (pre-tag review of 0.4.34): FEM's exit path called the trait default
//! `stop_for_exit()` for every enabled integration, which for SpaceAcres is
//! `stop()`: an image-name sweep that force-killed a farmer FEM never started
//! (one Windows autostarted at login) on every ordinary quit. 0.4.33 left it
//! running. On exit FEM stops only the instance it spawned itself.
//!
//! Real processes, real `stop()` sweep, dispatched through the trait as
//! main.rs does. The decoy carries the partner's image name and nothing else.

use super::SpaceAcresIntegration;
use crate::integrations::Integration;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

#[cfg(not(target_os = "windows"))]
fn decoy(dir: &std::path::Path) -> Child {
    // A symlink to the shell, not a written script: writing an executable and
    // exec'ing it from a multi-threaded test binary races concurrent forks
    // that briefly hold the write fd (ETXTBSY, "Text file busy"). The process
    // name (comm) is the link's name, which is what the image sweep matches.
    let path: PathBuf = dir.join("space-acres");
    let _ = std::fs::remove_file(&path);
    std::os::unix::fs::symlink("/bin/sh", &path).expect("decoy symlink");
    Command::new(&path)
        .args(["-c", "while :; do sleep 1; done"])
        .spawn()
        .expect("spawn decoy")
}

#[cfg(target_os = "windows")]
fn decoy(dir: &std::path::Path) -> Child {
    let path: PathBuf = dir.join("space-acres.exe");
    std::fs::copy(r"C:\Windows\System32\PING.EXE", &path).expect("copy decoy");
    Command::new(&path)
        .args(["-n", "120", "127.0.0.1"])
        .spawn()
        .expect("spawn decoy")
}

/// Whether `child` is still running after the image sweep had time to land.
fn alive_after(child: &mut Child) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if child.try_wait().expect("try_wait").is_some() {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    true
}

fn reap(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[tokio::test(flavor = "multi_thread")]
async fn quitting_fem_leaves_a_space_acres_it_did_not_start_running() {
    let dir = tempfile::tempdir().unwrap();
    let sa = SpaceAcresIntegration::default();
    let integration: &dyn Integration = &sa;

    // Positive control: the image-name sweep in stop() does reach this decoy.
    // D9: the sweep is machine-wide, so it runs only while no SpaceAcres this
    // test binary did not start is visible; otherwise the control fails loudly
    // instead of killing the host's own SpaceAcres.
    let mut control = decoy(dir.path());
    std::thread::sleep(Duration::from_millis(300));
    #[cfg(not(target_os = "windows"))]
    let foreign: Vec<u32> = std::fs::read_dir("/proc")
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let stat = std::fs::read_to_string(e.path().join("stat")).ok()?;
                    let (name, rest) = stat.split_once(" (")?.1.rsplit_once(") ")?;
                    let ppid: u32 = rest.split_whitespace().nth(1)?.parse().ok()?;
                    let pid: u32 = e.file_name().to_str()?.parse().ok()?;
                    (name == "space-acres" && ppid != std::process::id()).then_some(pid)
                })
                .collect()
        })
        .unwrap_or_else(|_| vec![0]);
    #[cfg(target_os = "windows")]
    let foreign: Vec<u32> = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "Get-CimInstance Win32_Process | Where-Object {{ @('space-acres.exe','space-acres-modern.exe') -contains $_.Name -and $_.ParentProcessId -ne {} }} | ForEach-Object {{ $_.ProcessId }}",
                std::process::id()
            ),
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| l.trim().parse().ok())
                .collect()
        })
        .unwrap_or_else(|| vec![0]);
    if foreign.is_empty() {
        integration.stop().await.unwrap();
    } else {
        eprintln!("D9: not sweeping; a SpaceAcres this test did not start is running: {foreign:?}");
    }
    assert!(
        !alive_after(&mut control),
        "control: stop()'s image sweep must reach the decoy"
    );

    let mut adopted = decoy(dir.path());
    std::thread::sleep(Duration::from_millis(300));
    integration.stop_for_exit().await.unwrap();
    let survived = alive_after(&mut adopted);
    reap(adopted);
    assert!(
        survived,
        "FAIL-6: quitting FEM force-killed a SpaceAcres FEM never started"
    );
}

/// SUPPORTING (green before and after): the instance FEM spawned is still
/// stopped on exit.
#[tokio::test(flavor = "multi_thread")]
async fn quitting_fem_still_stops_the_space_acres_it_spawned() {
    let dir = tempfile::tempdir().unwrap();
    let sa = SpaceAcresIntegration::default();
    let tracked = decoy(dir.path());
    let pid = tracked.id();
    // Hand FEM the handle, as its own start() does.
    *sa.child.lock().unwrap() = Some(tracked);
    let integration: &dyn Integration = &sa;
    integration.stop_for_exit().await.unwrap();
    assert!(
        sa.child.lock().unwrap().is_none(),
        "the tracked handle is consumed"
    );
    assert!(
        !pid_alive(pid),
        "the FEM-spawned instance must be stopped on exit"
    );
}

#[cfg(not(target_os = "windows"))]
fn pid_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
        && !std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .map(|s| s.contains(") Z "))
            .unwrap_or(true)
}

#[cfg(target_os = "windows")]
fn pid_alive(pid: u32) -> bool {
    let out = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
}
