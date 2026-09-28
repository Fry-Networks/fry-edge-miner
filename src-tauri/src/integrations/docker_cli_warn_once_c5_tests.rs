//! c5 D12: "Docker CLI is not on this process's PATH — using the installed
//! copy" was logged at WARN on EVERY re-resolution of the CLI. The lab probe
//! (c5-probes-rc18-w11: Docker Desktop installed while FEM ran) logged the
//! same path at 05:52:26.99, 06:02:27.00 and 06:12:32.49Z, one per 600 s
//! cache refresh. The fact is news once per state: when an installed copy is
//! first used, when the copy in use changes, or when it is used again after
//! the CLI was back on PATH. A refresh that finds the same copy is not.
//!
//! This drives `note_resolved_cli` with its own state (the process-wide one
//! is shared with every docker spawn in the suite) and counts the WARN lines
//! a subscriber actually captures, so the proof is about the log file.

use super::*;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;

const INSTALLED: &str = r"C:\Program Files\Docker\Docker\resources\bin\docker.exe";
const PER_USER: &str =
    r"C:\Users\femqa\AppData\Local\Programs\Docker\Docker\resources\bin\docker.exe";

#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);
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

/// Resolve the CLI once per entry of `sequence` (`None` = the bare name is
/// on PATH), and return how many "not on PATH" WARN lines were logged.
fn warns_for(sequence: &[Option<&str>]) -> usize {
    let state: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);
    let sink = Shared::default();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_writer(sink.clone())
            .with_ansi(false)
            .with_filter(tracing_subscriber::filter::LevelFilter::WARN),
    );
    tracing::subscriber::with_default(subscriber, || {
        for resolved in sequence.iter().copied() {
            note_resolved_cli(&state, resolved.map(Path::new));
        }
    });
    let text = String::from_utf8_lossy(&sink.0.lock().unwrap()).into_owned();
    text.matches("Docker CLI is not on this process's PATH")
        .count()
}

/// The lab probe's shape: the same installed copy at every 600 s refresh,
/// and again after a failed spawn dropped the cache.
#[test]
fn the_same_installed_copy_is_warned_once_not_every_refresh() {
    assert_eq!(
        warns_for(&[
            None,
            Some(INSTALLED),
            Some(INSTALLED),
            Some(INSTALLED),
            Some(INSTALLED)
        ]),
        1
    );
}

#[test]
fn a_different_installed_copy_is_warned_again() {
    assert_eq!(
        warns_for(&[
            Some(INSTALLED),
            Some(INSTALLED),
            Some(PER_USER),
            Some(PER_USER)
        ]),
        2
    );
}

/// Back on PATH, then off it again: a new state, so a new WARN.
#[test]
fn an_installed_copy_used_again_after_the_cli_was_on_path_is_warned_again() {
    assert_eq!(
        warns_for(&[
            Some(INSTALLED),
            None,
            None,
            Some(INSTALLED),
            Some(INSTALLED)
        ]),
        2
    );
}

#[test]
fn the_bare_name_on_path_never_warns() {
    assert_eq!(warns_for(&[None, None, None]), 0);
}
