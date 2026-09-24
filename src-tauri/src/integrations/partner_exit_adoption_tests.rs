//! FAIL-6 family (pre-tag review of 0.4.34): FEM's exit path runs each enabled
//! integration's `stop_for_exit()`, whose trait default is `stop()`. For
//! partners that stop by IMAGE NAME that meant force-killing instances FEM
//! never started on every ordinary quit (0.4.33 left them all running):
//! Storj's storagenode, which FEM never starts at all, and an OlostepBrowser the
//! owner or its own autostart launched. On exit FEM stops only what it spawned.

use crate::integrations::Integration;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

#[cfg(not(target_os = "windows"))]
fn decoy(dir: &Path, image: &str) -> Child {
    // A symlink to the shell, not a written script: writing an executable and
    // exec'ing it from a multi-threaded test binary races concurrent forks
    // that briefly hold the write fd (ETXTBSY, "Text file busy"). The process
    // name (comm) is the link's name, which is what the image sweep matches.
    let path: PathBuf = dir.join(image);
    let _ = std::fs::remove_file(&path);
    std::os::unix::fs::symlink("/bin/sh", &path).expect("decoy symlink");
    Command::new(&path)
        .args(["-c", "while :; do sleep 1; done"])
        .spawn()
        .expect("spawn decoy")
}

#[cfg(target_os = "windows")]
fn decoy(dir: &Path, image: &str) -> Child {
    let path: PathBuf = dir.join(format!("{image}.exe"));
    std::fs::copy(r"C:\Windows\System32\PING.EXE", &path).expect("copy decoy");
    Command::new(&path)
        .args(["-n", "120", "127.0.0.1"])
        .spawn()
        .expect("spawn decoy")
}

/// Whether `child` is still running once a sweep has had `grace` to land.
fn alive_after(child: &mut Child, grace: Duration) -> bool {
    let deadline = Instant::now() + grace;
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
async fn quitting_fem_never_kills_a_storagenode_fem_never_starts() {
    let dir = tempfile::tempdir().unwrap();
    let storj = super::storj::StorjIntegration;
    let integration: &dyn Integration = &storj;

    // Positive control: Storj's stop() sweep does reach the decoy.
    let mut control = decoy(dir.path(), "storagenode");
    std::thread::sleep(Duration::from_millis(300));
    integration.stop().await.unwrap();
    assert!(
        !alive_after(&mut control, Duration::from_secs(3)),
        "control: stop()'s image sweep must reach the decoy"
    );

    let mut node = decoy(dir.path(), "storagenode");
    std::thread::sleep(Duration::from_millis(300));
    integration.stop_for_exit().await.unwrap();
    let survived = alive_after(&mut node, Duration::from_secs(3));
    reap(node);
    assert!(
        survived,
        "FAIL-6: quitting FEM force-killed a storagenode FEM never started"
    );
}

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Off Windows `AemIntegration::stop()` touches no process, so the Olostep
/// decision is pinned on the real source here and behaviourally on Windows
/// below.
#[test]
fn olostep_exit_stops_only_the_instance_fem_spawned() {
    let code = code_only(include_str!("aem.rs"));
    let imp = code
        .find("impl Integration for AemIntegration")
        .expect("control: the trait impl must exist");
    let trait_impl = &code[imp..];
    assert!(
        trait_impl.contains("async fn stop(&self)"),
        "control: the trait impl must hold stop()"
    );
    let at = trait_impl
        .find("async fn stop_for_exit(&self)")
        .unwrap_or_else(|| {
            panic!(
                "FAIL-6: Olostep has no exit-specific stop, so quitting FEM runs the \
             image-name taskkill against an OlostepBrowser FEM never started"
            )
        });
    let body = &trait_impl[at..at + trait_impl[at..].find("\n    }\n").unwrap()];
    let tracked = body
        .find("self.child")
        .unwrap_or_else(|| panic!("FAIL-6: the exit stop must consult the tracked child:\n{body}"));
    let stop = body.find("self.stop().await").unwrap_or_else(|| {
        panic!("FAIL-6: the FEM-spawned instance must still be stopped:\n{body}")
    });
    assert!(
        tracked < stop && body[..stop].contains("if "),
        "FAIL-6: stop() must run only when FEM holds the tracked child:\n{body}"
    );
}

#[cfg(target_os = "windows")]
#[tokio::test(flavor = "multi_thread")]
async fn quitting_fem_leaves_an_olostep_it_did_not_start_running() {
    let dir = tempfile::tempdir().unwrap();
    let aem = super::aem::AemIntegration::default();
    let integration: &dyn Integration = &aem;

    let mut control = decoy(dir.path(), "OlostepBrowser");
    std::thread::sleep(Duration::from_millis(300));
    integration.stop().await.unwrap();
    assert!(
        !alive_after(&mut control, Duration::from_secs(3)),
        "control: stop()'s image sweep must reach the decoy"
    );

    let mut adopted = decoy(dir.path(), "OlostepBrowser");
    std::thread::sleep(Duration::from_millis(300));
    integration.stop_for_exit().await.unwrap();
    let survived = alive_after(&mut adopted, Duration::from_secs(3));
    reap(adopted);
    assert!(
        survived,
        "FAIL-6: quitting FEM killed an OlostepBrowser FEM never started"
    );
}
