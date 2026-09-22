//! Opt-in, scrubbed debug logging to a directory the user can find and share.
//!
//! ## Why this captures DEBUG *and above*, not DEBUG only
//!
//! The crate has 7 `debug!` call sites against 203 `info!` and 159 `warn!`.
//! Integration health checks, supervisor spawn/crash/restart, API summaries and
//! PoC submissions — everything a support bundle actually needs — are logged at
//! INFO. A DEBUG-only sink would be nearly empty and would contain none of it.
//!
//! So this sink is a superset of `fem.log`. Its user-facing value is not extra
//! verbosity, it is that **every line is scrubbed before it touches disk**.
//! `fem.log` holds raw tracing output and is only scrubbed when
//! `export_debug_bundle` zips it; this gives a continuously shareable capture.
//!
//! ## Why the toggle is enforced in the writer, not in a filter
//!
//! `tracing` caches per-callsite `Interest`. A filter that answers "no" while
//! the toggle is off can have that answer cached, so flipping the toggle on
//! later changes nothing and the feature is silently dead. Gating inside the
//! `MakeWriter` cannot suffer that: the layer always stays interested, and the
//! writer discards while disabled. The cost is formatting events that are then
//! dropped, which is what `fem.log` already pays for the same events.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

use super::scrubber::scrub_line;

/// Subdirectory of the app log dir. Deliberately a sibling of `fem.log` rather
/// than a second root under %APPDATA%: support should only ever have to ask for
/// one location, and the UI shows the resolved absolute path anyway.
pub const DEBUG_LOG_DIRNAME: &str = "debug-logs";

/// Base name for the daily-rotated files (`fem-debug.log.YYYY-MM-DD`).
pub const DEBUG_LOG_BASENAME: &str = "fem-debug.log";

/// Files older than this are pruned at startup.
pub const DEBUG_LOG_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Same trap as `LOG_GUARD` in the parent module: drop this and the worker
/// thread dies, after which every line is silently discarded and the user sees
/// an empty directory that looks like a broken feature.
static DEBUG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// PURE: where the debug logs live, given the app log dir.
pub fn debug_log_dir(app_log_dir: &Path) -> PathBuf {
    app_log_dir.join(DEBUG_LOG_DIRNAME)
}

pub fn set_enabled(on: bool) {
    DEBUG_ENABLED.store(on, Ordering::SeqCst);
}

pub fn is_enabled() -> bool {
    DEBUG_ENABLED.load(Ordering::SeqCst)
}

/// Park the worker guard for the life of the process.
pub fn park_guard(guard: WorkerGuard) {
    let _ = DEBUG_GUARD.set(guard);
}

/// Build the rotating debug-log writer and its guard.
pub fn build_debug_writer(dir: &Path) -> io::Result<(NonBlocking, WorkerGuard)> {
    std::fs::create_dir_all(dir)?;
    let appender = tracing_appender::rolling::daily(dir, DEBUG_LOG_BASENAME);
    Ok(tracing_appender::non_blocking(appender))
}

/// A writer that scrubs every line it passes through, and — when `gated` —
/// discards while the debug toggle is off.
///
/// B23: `gated: false` is what lets the RELEASE sink reuse this exact write
/// path. `fem.log` was written with no scrubber at all, so a Windows username
/// reached the log folder through every partner binary path FEM logs (the
/// spawn event's `command` field is under `%APPDATA%`, i.e.
/// `C:\Users\<name>\AppData\Roaming`). The line handling below is not
/// duplicated for it — a second implementation is how two writers drift apart.
pub struct ScrubbingWriter<W> {
    inner: W,
    gated: bool,
}

impl<W> ScrubbingWriter<W> {
    /// Gated by the user's debug-logging toggle — the debug sink's behaviour,
    /// unchanged.
    pub fn new(inner: W) -> Self {
        Self { inner, gated: true }
    }

    /// Always writes, still scrubbing. For the main sink, which is not
    /// optional.
    pub fn always(inner: W) -> Self {
        Self {
            inner,
            gated: false,
        }
    }
}

impl<W: Write> Write for ScrubbingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.gated && !is_enabled() {
            // Report the bytes as consumed. Returning 0 would look like a stuck
            // writer to the caller and could spin it.
            return Ok(buf.len());
        }

        let text = String::from_utf8_lossy(buf);
        let mut out = String::with_capacity(text.len());
        // `split_inclusive` keeps the terminator, so a buffer that does not end
        // in a newline stays un-terminated rather than gaining one. The
        // terminator is held back across `scrub_line` because the redactors
        // are line-oriented and several are anchored on trailing context.
        for chunk in text.split_inclusive('\n') {
            match chunk.strip_suffix('\n') {
                Some(body) => {
                    out.push_str(&scrub_line(body));
                    out.push('\n');
                }
                None => out.push_str(&scrub_line(chunk)),
            }
        }
        self.inner.write_all(out.as_bytes())?;
        // Consumed all of `buf`; the scrubbed form is a different length and
        // reporting that length would corrupt the caller's accounting.
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// `MakeWriter` wrapper so the fmt layer gets a fresh `ScrubbingWriter` per event.
#[derive(Clone)]
pub struct ScrubbingMakeWriter {
    inner: NonBlocking,
    gated: bool,
}

impl ScrubbingMakeWriter {
    pub fn new(inner: NonBlocking) -> Self {
        Self {
            inner,
            gated: true,
        }
    }

    /// B23: the main sink's writer — scrubs, and is never gated by the
    /// debug-logging toggle.
    pub fn always(inner: NonBlocking) -> Self {
        Self {
            inner,
            gated: false,
        }
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for ScrubbingMakeWriter {
    type Writer = ScrubbingWriter<NonBlocking>;

    fn make_writer(&'a self) -> Self::Writer {
        if self.gated {
            ScrubbingWriter::new(self.inner.clone())
        } else {
            ScrubbingWriter::always(self.inner.clone())
        }
    }
}

/// PURE: which of these `(name, age)` entries are past the retention window.
pub fn stale_entries(entries: &[(String, Duration)], max_age: Duration) -> Vec<String> {
    entries
        .iter()
        .filter(|(_, age)| *age > max_age)
        .map(|(name, _)| name.clone())
        .collect()
}

/// Delete debug logs older than `max_age`. Returns how many were removed.
pub fn prune_debug_logs(dir: &Path, max_age: Duration) -> io::Result<usize> {
    if !dir.is_dir() {
        return Ok(0);
    }
    let now = std::time::SystemTime::now();
    let mut entries: Vec<(String, Duration)> = Vec::new();
    for e in std::fs::read_dir(dir)?.flatten() {
        if !e.path().is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        let age = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .unwrap_or_default();
        entries.push((name, age));
    }
    let mut removed = 0;
    for name in stale_entries(&entries, max_age) {
        if std::fs::remove_file(dir.join(&name)).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex, MutexGuard};

    /// `DEBUG_ENABLED` is process-global, so tests that flip it must not run
    /// concurrently. Restores the previous value in `Drop` — a panic skips a
    /// cleanup line at the end of the body, and this guard is NOT re-entrant.
    static TOGGLE_LOCK: Mutex<()> = Mutex::new(());

    struct ToggleGuard {
        _lock: MutexGuard<'static, ()>,
        previous: bool,
    }

    impl ToggleGuard {
        fn acquire(initial: bool) -> Self {
            let lock = TOGGLE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let previous = is_enabled();
            set_enabled(initial);
            Self {
                _lock: lock,
                previous,
            }
        }
    }

    impl Drop for ToggleGuard {
        fn drop(&mut self) {
            set_enabled(self.previous);
        }
    }

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Sink {
        fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
        }
    }

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn write_through(sink: &Sink, line: &str) {
        let mut w = ScrubbingWriter::new(sink.clone());
        w.write_all(line.as_bytes()).expect("write");
        w.flush().expect("flush");
    }

    fn write_through_always(sink: &Sink, line: &str) {
        let mut w = ScrubbingWriter::always(sink.clone());
        w.write_all(line.as_bytes()).expect("write");
        w.flush().expect("flush");
    }

    /// B23: the main sink is NOT optional. `fem.log` was written with no
    /// scrubber at all, so a Windows username reached the log folder through
    /// every partner binary path FEM logs. Reusing this writer with the gate
    /// off is what fixes that without a second implementation of the line
    /// handling.
    #[test]
    fn an_always_on_scrubbing_writer_writes_while_the_toggle_is_off() {
        let _guard = ToggleGuard::acquire(false);
        let sink = Sink::default();
        write_through_always(
            &sink,
            "Spawning process command=\"C:\\Users\\georgep\\AppData\\Roaming\\x.exe\"\n",
        );
        let text = sink.text();
        assert!(
            text.contains("Spawning process"),
            "the main sink must write regardless of the debug toggle: {text:?}"
        );
        assert!(
            !text.contains("georgep"),
            "and it must still scrub: {text:?}"
        );
    }

    /// …while the DEBUG sink's gate is unchanged.
    #[test]
    fn a_gated_scrubbing_writer_still_discards_while_the_toggle_is_off() {
        let _guard = ToggleGuard::acquire(false);
        let sink = Sink::default();
        write_through(&sink, "this line must not be written\n");
        assert_eq!(
            sink.text(),
            "",
            "the opt-in sink must stay opt-in"
        );
    }

    /// These logs are written so a user can hand the folder to support. A raw
    /// wallet address or a `C:\Users\<real name>` path in one is a leak the
    /// user cannot see before sharing.
    #[test]
    fn sensitive_values_are_redacted_before_they_reach_the_sink() {
        let _g = ToggleGuard::acquire(true);
        let sink = Sink::default();
        let addr = "A".repeat(58);
        write_through(
            &sink,
            &format!("INFO paying out to {addr} from D:\\Users\\jdoe\\fem\n"),
        );

        let out = sink.text();
        assert!(
            !out.contains(&addr),
            "the full Algorand address must not reach disk: {out:?}"
        );
        assert!(
            !out.contains("jdoe"),
            "the username must not reach disk: {out:?}"
        );
    }

    /// The must-survive control. A redactor that eats everything passes every
    /// one-way "is the secret gone?" assertion, so the absence test above only
    /// means something paired with this.
    #[test]
    fn an_ordinary_line_comes_through_unchanged() {
        let _g = ToggleGuard::acquire(true);
        let sink = Sink::default();
        let line = "INFO supervisor: restarting partner process after exit code 1\n";
        write_through(&sink, line);
        assert_eq!(
            sink.text(),
            line,
            "a line with nothing sensitive in it must survive byte-identical"
        );
    }

    /// Off is the default and must mean *nothing on disk*, and the toggle has
    /// to take effect within the running process — a setting that needs a
    /// restart to do anything is not what the Settings switch promises.
    #[test]
    fn the_toggle_gates_writes_and_takes_effect_immediately() {
        let _g = ToggleGuard::acquire(false);
        let sink = Sink::default();

        write_through(&sink, "while-off\n");
        assert_eq!(sink.text(), "", "nothing may be written while disabled");

        set_enabled(true);
        write_through(&sink, "while-on\n");
        assert!(
            sink.text().contains("while-on"),
            "enabling must start writing without a restart"
        );

        set_enabled(false);
        write_through(&sink, "after-off\n");
        assert!(
            !sink.text().contains("after-off"),
            "disabling must stop writing without a restart"
        );
    }

    #[test]
    fn files_past_the_retention_window_are_pruned_and_recent_ones_kept() {
        let day = Duration::from_secs(86_400);
        let entries = vec![
            ("fem-debug.log.2026-09-01".to_string(), day * 13),
            ("fem-debug.log.2026-09-06".to_string(), day * 8),
            ("fem-debug.log.2026-09-12".to_string(), day * 2),
            (
                "fem-debug.log.2026-09-14".to_string(),
                Duration::from_secs(60),
            ),
        ];
        let stale = stale_entries(&entries, DEBUG_LOG_MAX_AGE);
        assert_eq!(
            stale,
            vec![
                "fem-debug.log.2026-09-01".to_string(),
                "fem-debug.log.2026-09-06".to_string()
            ],
            "only files older than 7 days may be pruned"
        );
    }

    /// Exercises the real filesystem path (read_dir -> metadata -> remove).
    /// The window is varied instead of the files' mtimes, because backdating an
    /// mtime would mean pulling in a new dependency for one assertion.
    #[test]
    fn prune_deletes_through_to_the_real_directory_and_spares_fresh_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = dir.path().join("fem-debug.log.2026-09-01");
        let b = dir.path().join("fem-debug.log.2026-09-14");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();

        // Must-survive control: with the real 7-day window, nothing just
        // written can qualify. A prune that deletes here is deleting blind.
        let kept = prune_debug_logs(dir.path(), DEBUG_LOG_MAX_AGE).expect("prune");
        assert_eq!(kept, 0, "freshly written files must not be pruned");
        assert!(a.exists() && b.exists());

        // And with a zero-length window every file is past it, which proves the
        // deletion actually reaches disk rather than only counting.
        let removed = prune_debug_logs(dir.path(), Duration::ZERO).expect("prune");
        assert_eq!(removed, 2, "both files are past a zero-length window");
        assert!(!a.exists() && !b.exists(), "files must actually be removed");
    }

    /// Mirrors `dropping_the_guard_first_silently_discards_everything` in the
    /// parent module. The new sink has its own guard and the same trap.
    #[test]
    fn the_debug_writer_needs_its_guard_held_to_deliver_anything() {
        let _g = ToggleGuard::acquire(true);
        let dir = tempfile::tempdir().expect("tempdir");
        let (writer, guard) = build_debug_writer(dir.path()).expect("writer");
        drop(guard);

        let mut w = ScrubbingWriter::new(writer);
        let _ = w.write_all(b"guard-dropped-debug-marker\n");
        let _ = w.flush();

        let mut text = String::new();
        for e in std::fs::read_dir(dir.path()).unwrap().flatten() {
            if let Ok(s) = std::fs::read_to_string(e.path()) {
                text.push_str(&s);
            }
        }
        assert!(
            !text.contains("guard-dropped-debug-marker"),
            "a dropped guard must not deliver lines, got: {text:?}"
        );
    }

    #[test]
    fn the_debug_directory_sits_beside_the_main_log() {
        let root = Path::new("C:/Users/x/AppData/Local/com.frynetworks.fem/logs");
        assert_eq!(debug_log_dir(root), root.join("debug-logs"));
    }
}
