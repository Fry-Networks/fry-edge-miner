//! FAIL-14 (row 8): `docker_bounded_probe`'s CLI-spawn-failure WARN fired on
//! every probe — a real 76-minute soak captured 175 identical "Docker CLI
//! could not be spawned" lines, one per health tick, from
//! `docker_manager.rs:183-188`'s unconditional `warn!(...)` in the spawn
//! `Err` arm.
//!
//! This drives the REAL, unmodified production function
//! (`docker_bounded_probe`) through repeated CLI-spawn failures and counts
//! the actual WARN lines a subscriber captures — not an internal counter —
//! so the proof is about what a shipped log file would contain. The CLI path
//! is seeded straight into `DOCKER_CLI_CACHE` as a definitely-nonexistent
//! absolute path before every attempt, so the spawn failure is deterministic
//! regardless of whether `docker` happens to be installed on the runner (it
//! is, on this dev box) — `docker_command()`'s freshness check then skips the
//! real on-PATH probe entirely and returns that bogus path straight away.

use super::*;
use std::sync::{Arc, Mutex};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;

const BOGUS_DOCKER_CLI: &str = "/nonexistent/femqa-fail14-fake-docker-binary";

/// Point `docker_command()`'s cache straight at a path that cannot exist,
/// with a fresh timestamp, so the very next `docker_bounded_probe` call
/// spawns it (deterministic failure) without probing the real PATH.
fn seed_bogus_docker_cli_cache() {
    let mut guard = DOCKER_CLI_CACHE.lock().expect("cache lock");
    *guard = Some((
        Some(PathBuf::from(BOGUS_DOCKER_CLI)),
        std::time::Instant::now(),
    ));
}

/// A definitely-spawnable binary that exits immediately and ignores its
/// arguments — used to produce a real spawn SUCCESS deterministically,
/// without depending on the real `docker` CLI.
const SPAWNABLE_NOOP: &str = "/bin/true";

fn seed_spawnable_docker_cli_cache() {
    let mut guard = DOCKER_CLI_CACHE.lock().expect("cache lock");
    *guard = Some((
        Some(PathBuf::from(SPAWNABLE_NOOP)),
        std::time::Instant::now(),
    ));
}

#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);
impl Shared {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}
impl std::io::Write for Shared {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Shared {
    type Writer = Shared;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn warn_count(text: &str) -> usize {
    text.matches("Docker CLI could not be spawned").count()
}

/// Drive a real spawn SUCCESS (outside any capturing subscriber) so the
/// "previous spawn failed?" state starts each test from a known baseline —
/// state left over from whichever of these tests ran first, since
/// `DOCKER_SPAWN_FAILED` is a process-global static that outlives any one
/// test. Uses the product's own success path (not a direct poke at the
/// static) so this stays correct if that internal ever changes shape.
fn reset_spawn_state_to_not_previously_failed() {
    seed_spawnable_docker_cli_cache();
    let probe = docker_bounded_probe(&["--version"], 5);
    assert_ne!(
        probe,
        DockerProbe::CliMissing,
        "the seeded /bin/true must spawn successfully"
    );
}

/// `DOCKER_CLI_CACHE` and `DOCKER_SPAWN_FAILED` are PROCESS-GLOBAL statics —
/// the same ones the real probe uses — so the two tests below that drive them
/// through a specific failure/success sequence must not run concurrently with
/// each other (cargo test runs tests in parallel threads by default). No
/// other test in this crate touches either static.
static TEST_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serialized() -> std::sync::MutexGuard<'static, ()> {
    TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn capturing_subscriber(sink: &Shared) -> impl tracing::Subscriber {
    tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_writer(sink.clone())
            .with_ansi(false)
            .with_filter(tracing_subscriber::filter::LevelFilter::WARN),
    )
}

#[test]
fn the_bogus_cli_path_really_is_unspawnable() {
    assert!(
        std::process::Command::new(BOGUS_DOCKER_CLI)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_err(),
        "fixture sanity: {BOGUS_DOCKER_CLI} must not exist on the test runner"
    );
}

/// N consecutive failed probes must log the fact exactly once.
#[test]
fn n_consecutive_spawn_failures_log_exactly_one_warn() {
    let _guard = serialized();
    reset_spawn_state_to_not_previously_failed();
    let sink = Shared::default();
    let subscriber = capturing_subscriber(&sink);

    tracing::subscriber::with_default(subscriber, || {
        for _ in 0..175 {
            seed_bogus_docker_cli_cache();
            let probe = docker_bounded_probe(&["info"], 1);
            assert_eq!(probe, DockerProbe::CliMissing);
        }
    });

    assert_eq!(
        warn_count(&sink.text()),
        1,
        "175 consecutive CLI-spawn failures must log exactly one WARN, got:\n{}",
        sink.text()
    );
}

/// A real spawn success between two failure runs is a state change: the
/// failure after it is a NEW fact and must warn again.
#[test]
fn a_failure_after_a_success_warns_again() {
    let _guard = serialized();
    reset_spawn_state_to_not_previously_failed();
    let sink = Shared::default();
    let subscriber = capturing_subscriber(&sink);

    tracing::subscriber::with_default(subscriber, || {
        // Three failures: exactly one WARN.
        for _ in 0..3 {
            seed_bogus_docker_cli_cache();
            docker_bounded_probe(&["info"], 1);
        }
        // A real spawn success resets the tracked state.
        seed_spawnable_docker_cli_cache();
        let probe = docker_bounded_probe(&["--version"], 5);
        assert_ne!(
            probe,
            DockerProbe::CliMissing,
            "the seeded /bin/true must spawn successfully"
        );

        // One more failure: a NEW transition, must warn again.
        seed_bogus_docker_cli_cache();
        docker_bounded_probe(&["info"], 1);
    });

    assert_eq!(
        warn_count(&sink.text()),
        2,
        "a success between two failure runs must reset the state, so the \
         failure after it logs its own WARN — got:\n{}",
        sink.text()
    );
}
