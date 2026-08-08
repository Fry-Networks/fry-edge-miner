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

    let add = |zip: &mut ZipWriter<std::fs::File>, path: &PathBuf, name: &str| -> Result<(), String> {
        let Ok(contents) = fs::read(path) else { return Ok(()) };
        let contents_str = String::from_utf8_lossy(&contents);
        let scrubbed = contents_str
            .lines()
            .map(scrubber::scrub_line)
            .collect::<Vec<_>>()
            .join("\n");
        let options: zip::write::FileOptions<()> = Default::default();
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(scrubbed.as_bytes()).map_err(|e| e.to_string())?;
        Ok(())
    };

    // Add log files if found. The supervisor writes each partner's stdout/stderr
    // into a per-integration subdirectory, so recurse one level — a top-level
    // only walk shipped bundles without any partner diagnostics.
    if let Some(log_path) = log_dir {
        if log_path.exists() {
            if let Ok(entries) = fs::read_dir(&log_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let Ok(metadata) = entry.metadata() else { continue };
                    if metadata.is_file() {
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("log").to_string();
                        add(&mut zip, &path, &name)?;
                    } else if metadata.is_dir() {
                        let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("integration").to_string();
                        if let Ok(sub) = fs::read_dir(&path) {
                            for sub_entry in sub.flatten() {
                                let sub_path = sub_entry.path();
                                if sub_path.is_file() {
                                    let leaf = sub_path.file_name().and_then(|n| n.to_str()).unwrap_or("log");
                                    add(&mut zip, &sub_path, &format!("{}/{}", dir_name, leaf))?;
                                }
                            }
                        }
                    }
                }
            }
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
}
