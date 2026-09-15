pub mod debug_sink;
pub mod scrubber;

use std::path::Path;
use std::sync::OnceLock;

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

/// Keeps the non-blocking writer's worker thread alive for the life of the
/// process.
///
/// `tracing_appender::non_blocking` hands back a `WorkerGuard` that owns the
/// background writer thread; when the guard drops, the thread shuts down and
/// every subsequent log line is silently discarded. Binding it to a local
/// inside `init_logging` meant it dropped on return, so release builds created
/// a `fem.log.<date>` file every day and never wrote a byte to any of them.
#[cfg_attr(debug_assertions, allow(dead_code))] // dev builds log to stdout
static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// Build the rotating-file writer and its guard.
///
/// Deliberately compiled in every profile — only its *use* is release-only —
/// so the guard-lifetime behaviour can be tested by a normal `cargo test` run.
#[cfg_attr(debug_assertions, allow(dead_code))] // used by the release branch and by tests
pub(crate) fn build_file_writer(log_dir: &Path) -> std::io::Result<(NonBlocking, WorkerGuard)> {
    std::fs::create_dir_all(log_dir)?;
    let file_appender = tracing_appender::rolling::daily(log_dir, "fem.log");
    Ok(tracing_appender::non_blocking(file_appender))
}

/// Initialize logging: stdout in dev, daily rotating files in release.
///
/// Note that scrubbing is applied when a debug bundle is exported
/// (`commands::debug::export_debug_bundle`), not at write time — the file on
/// disk holds raw tracing output. The scrubber redacts:
/// - 25-word BIP39 mnemonics → [MNEMONIC]
/// - bearer/api_key/token/OP_* → [REDACTED]
/// - 58-char base32 Algorand addresses → first4…last4
/// - IPv4 → mask last octet
/// - MACs → [MAC]
/// - Usernames → <user>
/// - Hostnames → <host>
/// - Serial-like long hex → [SERIAL]
pub fn init_logging(log_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use tracing_subscriber::fmt::format::FmtSpan;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::Layer;

    // The env filter is now a PER-LAYER filter on the main sink rather than a
    // global one. That keeps `fem.log` behaving exactly as before (including
    // RUST_LOG overrides) while still letting the debug layer see DEBUG events,
    // which a global "info" filter would discard before any layer ran.
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    // Opt-in debug sink. Always constructed; the toggle is enforced inside its
    // writer, not by adding or removing this layer — see `debug_sink`.
    let debug_dir = debug_sink::debug_log_dir(log_dir);
    let debug_layer = match debug_sink::build_debug_writer(&debug_dir) {
        Ok((writer, guard)) => {
            // Same trap as LOG_GUARD: park it or every line is discarded.
            debug_sink::park_guard(guard);
            if let Err(e) = debug_sink::prune_debug_logs(&debug_dir, debug_sink::DEBUG_LOG_MAX_AGE)
            {
                eprintln!("Warning: could not prune old debug logs: {}", e);
            }
            Some(
                tracing_subscriber::fmt::layer()
                    .with_writer(debug_sink::ScrubbingMakeWriter::new(writer))
                    .with_ansi(false)
                    .with_span_events(FmtSpan::CLOSE)
                    .with_filter(tracing_subscriber::filter::LevelFilter::DEBUG),
            )
        }
        Err(e) => {
            // A debug sink that cannot be created must not take logging down
            // with it.
            eprintln!("Warning: debug logging unavailable: {}", e);
            None
        }
    };

    // Dev: stdout. Release: daily rotating files.
    #[cfg(debug_assertions)]
    let main_layer = tracing_subscriber::fmt::layer()
        .with_span_events(FmtSpan::CLOSE)
        .with_filter(env_filter);

    #[cfg(not(debug_assertions))]
    let main_layer = {
        let (non_blocking, guard) = build_file_writer(log_dir)?;
        // Park the guard for the process lifetime. Letting it drop here is
        // exactly the bug this replaces.
        let _ = LOG_GUARD.set(guard);
        tracing_subscriber::fmt::layer()
            .with_writer(non_blocking)
            // Without this the file gets terminal colour escapes — 372 of them
            // in a 22-line sample — which makes a support bundle painful to read.
            .with_ansi(false)
            .with_span_events(FmtSpan::CLOSE)
            .with_filter(env_filter)
    };

    tracing_subscriber::registry()
        .with(main_layer)
        .with(debug_layer)
        .init();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// Read every file the rolling appender produced in `dir`, concatenated.
    /// The daily appender names files `fem.log.<YYYY-MM-DD>`, so the exact name
    /// depends on today's date — collect whatever landed instead of guessing.
    fn collected(dir: &Path) -> String {
        let mut all = String::new();
        for entry in std::fs::read_dir(dir).expect("log dir readable").flatten() {
            if entry.path().is_file() {
                let mut s = String::new();
                if let Ok(mut f) = std::fs::File::open(entry.path()) {
                    let _ = f.read_to_string(&mut s);
                }
                all.push_str(&s);
            }
        }
        all
    }

    /// Emit one event through a writer, using a *scoped* subscriber so no
    /// global subscriber is installed — `init()` can only run once per process
    /// and would poison the rest of the suite.
    fn emit_through(writer: NonBlocking, message: &str) {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(writer)
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("{}", message);
        });
    }

    #[test]
    fn holding_the_guard_lets_log_lines_reach_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (writer, guard) = build_file_writer(dir.path()).expect("writer");

        emit_through(writer, "guard-held-marker");
        drop(guard); // dropping flushes the worker

        let text = collected(dir.path());
        assert!(
            text.contains("guard-held-marker"),
            "expected the log line on disk, got {} bytes: {:?}",
            text.len(),
            text
        );
    }

    #[test]
    fn dropping_the_guard_first_silently_discards_everything() {
        // This is the shipped defect, pinned: FEM created a fem.log file every
        // day and left all of them 0 bytes because the guard died on return
        // from init_logging. Kept so a refactor cannot quietly reintroduce it.
        let dir = tempfile::tempdir().expect("tempdir");
        let (writer, guard) = build_file_writer(dir.path()).expect("writer");
        drop(guard);

        emit_through(writer, "guard-dropped-marker");

        let text = collected(dir.path());
        assert!(
            !text.contains("guard-dropped-marker"),
            "a dropped guard must not deliver log lines, but got: {:?}",
            text
        );
    }

    /// The debug sink must be ADDITIVE. `init_logging` moved from a single
    /// `fmt()` subscriber to a layered registry, and the failure mode of that
    /// refactor is the new DEBUG-level layer dragging debug events into
    /// `fem.log` too — quietly changing what every shipped device writes.
    ///
    /// Asserts on the composition itself (two sinks, the real per-layer
    /// filters) rather than on `init_logging`, which installs a global
    /// subscriber and can only run once per process.
    #[test]
    fn debug_events_reach_the_debug_sink_but_never_the_main_log() {
        use std::sync::{Arc, Mutex};
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::Layer;

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

        let main_sink = Shared::default();
        let debug_sink_out = Shared::default();

        let subscriber = tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(main_sink.clone())
                    .with_ansi(false)
                    .with_filter(tracing_subscriber::EnvFilter::new("info")),
            )
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(debug_sink_out.clone())
                    .with_ansi(false)
                    .with_filter(tracing_subscriber::filter::LevelFilter::DEBUG),
            );

        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!("debug-only-marker");
            tracing::info!("info-marker");
        });

        let main = main_sink.text();
        let dbg = debug_sink_out.text();

        assert!(
            !main.contains("debug-only-marker"),
            "fem.log must keep its INFO floor, got: {main:?}"
        );
        assert!(
            main.contains("info-marker"),
            "fem.log must still receive INFO, got: {main:?}"
        );
        assert!(
            dbg.contains("debug-only-marker"),
            "the debug sink must receive DEBUG, got: {dbg:?}"
        );
        assert!(
            dbg.contains("info-marker"),
            "the debug sink is DEBUG-and-above, so INFO belongs in it too: {dbg:?}"
        );
    }

    #[test]
    fn creates_the_log_directory_when_it_does_not_exist() {
        let root = tempfile::tempdir().expect("tempdir");
        let nested = root.path().join("does").join("not").join("exist");
        assert!(!nested.exists());

        let (_writer, _guard) = build_file_writer(&nested).expect("writer");
        assert!(nested.is_dir(), "build_file_writer must create the log dir");
    }
}
