use crate::logging::scrubber;
use std::fs;
use std::path::PathBuf;

/// Export a debug bundle (recent logs + scrubbed system info) to a chosen path.
///
/// Returns the path where the bundle was written.
#[tauri::command]
pub async fn export_debug_bundle(
    destination: Option<String>,
    app: tauri::AppHandle,
    _state: tauri::State<'_, crate::AppState>,
) -> Result<String, String> {
    use std::io::Write;
    use tauri::Manager;
    use zip::ZipWriter;

    // Ask Tauri for the same directory init_logging writes to. This used to be
    // hardcoded to %APPDATA%\FryEdgeMiner\logs — wrong root and wrong name, so
    // the bundle silently shipped without a single log file. The real location
    // is %LOCALAPPDATA%\com.frynetworks.fem\logs.
    let log_dir: Option<PathBuf> = app.path().app_log_dir().ok();

    // No file-picker plugin is installed, so with no destination we drop the
    // bundle in Downloads and hand the caller the path to display.
    let dest_path = match destination.filter(|d| !d.trim().is_empty()) {
        Some(d) => PathBuf::from(d),
        None => {
            let dir = app
                .path()
                .download_dir()
                .map_err(|e| format!("Could not resolve a Downloads folder: {}", e))?;
            dir.join(format!("fry-edge-miner-debug-{}.zip", bundle_stamp()))
        }
    };
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let bundle_file = std::fs::File::create(&dest_path).map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(bundle_file);

    let add =
        |zip: &mut ZipWriter<std::fs::File>, path: &PathBuf, name: &str| -> Result<(), String> {
            let Ok(contents) = fs::read(path) else {
                return Ok(());
            };
            let contents_str = String::from_utf8_lossy(&contents);
            let scrubbed = contents_str
                .lines()
                .map(scrubber::scrub_line)
                .collect::<Vec<_>>()
                .join("\n");
            let options: zip::write::FileOptions<()> = Default::default();
            zip.start_file(name, options).map_err(|e| e.to_string())?;
            zip.write_all(scrubbed.as_bytes())
                .map_err(|e| e.to_string())?;
            Ok(())
        };

    // Add log files if found.
    if let Some(log_path) = log_dir {
        for (path, name) in bundle_entries(&log_path) {
            add(&mut zip, &path, &name)?;
        }
    }

    // Add scrubbed system info
    let sysinfo = collect_scrubbed_sysinfo();
    let options: zip::write::FileOptions<()> = Default::default();
    zip.start_file("sysinfo.txt", options)
        .map_err(|e| e.to_string())?;
    zip.write_all(sysinfo.as_bytes())
        .map_err(|e| e.to_string())?;

    zip.finish().map_err(|e| e.to_string())?;

    tracing::info!(path = %dest_path.display(), "Debug bundle exported");
    Ok(dest_path.to_string_lossy().to_string())
}

/// Every file the bundle should carry, as `(source path, name inside the zip)`.
///
/// Walks the app log directory and recurses **one level** into subdirectories.
/// That one level is load-bearing and carries two distinct payloads:
///
/// * the supervisor's per-integration stdout/stderr (`fryvpn/`, `iagon/`, …) —
///   a top-level-only walk once shipped bundles with no partner diagnostics at
///   all; and
/// * `debug-logs/`, the opt-in scrubbed capture from the Settings toggle, which
///   `debug_sink::debug_log_dir` deliberately places as a sibling of `fem.log`
///   so support only ever has to ask for one location.
///
/// The second of those was previously incidental — it worked only because this
/// walk happens to have no name filter. It is covered by tests now, so a future
/// change to the walk cannot quietly stop shipping it.
///
/// Entries are sorted so a bundle is reproducible; `read_dir` order is not
/// guaranteed. Unreadable directories yield nothing rather than failing the
/// export — a partial bundle beats no bundle when someone is reporting a bug.
fn bundle_entries(root: &std::path::Path) -> Vec<(PathBuf, String)> {
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    if !root.exists() {
        return out;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_file() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("log")
                .to_string();
            out.push((path, name));
        } else if metadata.is_dir() {
            let dir_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("integration")
                .to_string();
            if let Ok(sub) = fs::read_dir(&path) {
                for sub_entry in sub.flatten() {
                    let sub_path = sub_entry.path();
                    if sub_path.is_file() {
                        let leaf = sub_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("log");
                        let name = format!("{}/{}", dir_name, leaf);
                        out.push((sub_path, name));
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// Timestamp for the default bundle filename, so repeated exports don't
/// overwrite each other.
fn bundle_stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// Collect scrubbed system information (OS, device, config).
fn collect_scrubbed_sysinfo() -> String {
    let mut info = String::new();

    info.push_str("=== System Information (Scrubbed) ===\n\n");

    info.push_str(&format!("OS: {}\n", std::env::consts::OS));
    info.push_str(&format!("Architecture: {}\n", std::env::consts::ARCH));

    // Scrub env vars that might be relevant
    if let Ok(val) = std::env::var("COMPUTERNAME") {
        info.push_str(&format!("Computer: {}\n", scrubber::scrub_line(&val)));
    }

    if let Ok(val) = std::env::var("USERNAME") {
        info.push_str(&format!("User: {}\n", scrubber::scrub_line(&val)));
    }

    info.push_str("\n=== Build Information ===\n");
    info.push_str(&format!("Version: {}\n", env!("CARGO_PKG_VERSION")));
    info.push_str(&format!("Build time: {}\n", env!("CARGO_PKG_VERSION")));

    info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_scrubbed_sysinfo() {
        let info = collect_scrubbed_sysinfo();
        assert!(info.contains("=== System Information"));
        assert!(info.contains("OS:"));
        assert!(info.contains("Architecture:"));
        assert!(!info.is_empty());
    }

    /// Build a log directory shaped like a real one.
    fn log_tree(with_debug_logs: bool) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("fem.log.2026-09-14"), b"yesterday").unwrap();
        std::fs::write(root.join("fem.log.2026-09-15"), b"today").unwrap();
        std::fs::create_dir_all(root.join("fryvpn")).unwrap();
        std::fs::write(root.join("fryvpn").join("stdout.log"), b"partner").unwrap();
        if with_debug_logs {
            let d = root.join(crate::logging::debug_sink::DEBUG_LOG_DIRNAME);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("fem-debug.log.2026-09-15"), b"scrubbed capture").unwrap();
        }
        dir
    }

    /// The opt-in debug capture must reach the support zip. Users are told in
    /// Settings that this folder is what support wants; a bundle that omitted
    /// it would quietly contradict that.
    #[test]
    fn the_bundle_carries_the_debug_logs_directory() {
        let dir = log_tree(true);
        let names: Vec<String> = bundle_entries(dir.path())
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        assert!(
            names.contains(&"debug-logs/fem-debug.log.2026-09-15".to_string()),
            "debug-logs must be in the bundle, got {names:?}"
        );
    }

    /// The toggle ships off, so most bundles are produced with no debug-logs
    /// directory at all. That must stay an ordinary bundle, not an error.
    #[test]
    fn a_bundle_without_debug_logs_still_carries_everything_else() {
        let dir = log_tree(false);
        let names: Vec<String> = bundle_entries(dir.path())
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        assert!(
            !names.iter().any(|n| n.starts_with("debug-logs/")),
            "nothing may be invented when the directory is absent: {names:?}"
        );
        assert!(names.contains(&"fem.log.2026-09-15".to_string()));
        assert!(names.contains(&"fryvpn/stdout.log".to_string()));
    }

    /// Partner diagnostics live one level down too — the same recursion serves
    /// both, which is why it must not be narrowed to a single known directory.
    #[test]
    fn every_rotated_log_and_partner_subdirectory_is_included() {
        let dir = log_tree(true);
        let names: Vec<String> = bundle_entries(dir.path())
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        assert_eq!(
            names,
            vec![
                "debug-logs/fem-debug.log.2026-09-15".to_string(),
                "fem.log.2026-09-14".to_string(),
                "fem.log.2026-09-15".to_string(),
                "fryvpn/stdout.log".to_string(),
            ],
            "entries must be complete and sorted for a reproducible bundle"
        );
    }

    /// A log directory that does not exist yet is not an error — the export
    /// still produces sysinfo.txt.
    #[test]
    fn a_missing_log_directory_yields_no_entries_rather_than_failing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("not-created-yet");
        assert!(bundle_entries(&missing).is_empty());
    }

    /// The bundle re-scrubs every line on the way in, and the debug sink has
    /// already scrubbed its own. Double-scrubbing must be a no-op, or shared
    /// logs would degrade a little each time they passed through.
    #[test]
    fn scrubbing_already_scrubbed_text_changes_nothing() {
        let once = crate::logging::scrubber::scrub_line(
            r#"command="C:\\Users\\jdoe\\app.exe" addr=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"#,
        );
        let twice = crate::logging::scrubber::scrub_line(&once);
        assert_eq!(once, twice, "scrub_line must be idempotent");
        assert!(
            !once.contains("jdoe"),
            "sanity: the fixture must actually redact"
        );
    }
}

/// What the Settings page needs to render the Debug Logging section: where the
/// files go, and whether anything is being written there right now.
#[derive(Debug, serde::Serialize)]
pub struct DebugLogInfo {
    /// Absolute path, shown to the user so they can find the folder. Resolved
    /// rather than described, because a user cannot act on "%LOCALAPPDATA%".
    pub path: String,
    pub enabled: bool,
}

fn resolve_debug_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|e| format!("Could not resolve the log directory: {}", e))?;
    Ok(crate::logging::debug_sink::debug_log_dir(&log_dir))
}

#[tauri::command]
pub async fn get_debug_log_path(app: tauri::AppHandle) -> Result<DebugLogInfo, String> {
    Ok(DebugLogInfo {
        path: resolve_debug_dir(&app)?.to_string_lossy().into_owned(),
        enabled: crate::logging::debug_sink::is_enabled(),
    })
}

/// Flip debug logging and persist the choice.
///
/// The live toggle is flipped FIRST so the switch takes effect in this process
/// immediately; persistence only decides what happens after the next restart.
/// If the write fails the runtime state is rolled back, so what the UI reports
/// and what the process is doing cannot drift apart.
#[tauri::command]
pub async fn toggle_debug_logging(
    app: tauri::AppHandle,
    enabled: bool,
    state: tauri::State<'_, crate::AppState>,
) -> Result<DebugLogInfo, String> {
    let previous = crate::logging::debug_sink::is_enabled();
    crate::logging::debug_sink::set_enabled(enabled);

    if let Err(e) = state.config.update(|c| c.debug_logging_enabled = enabled) {
        crate::logging::debug_sink::set_enabled(previous);
        return Err(format!("Could not save the debug logging setting: {}", e));
    }

    tracing::info!(enabled, "Debug logging toggled");
    Ok(DebugLogInfo {
        path: resolve_debug_dir(&app)?.to_string_lossy().into_owned(),
        enabled,
    })
}
