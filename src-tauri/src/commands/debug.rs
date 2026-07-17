use crate::logging::scrubber;
use std::fs;
use std::path::PathBuf;

/// Export a debug bundle (recent logs + scrubbed system info) to a chosen path.
///
/// Returns the path where the bundle was written.
#[tauri::command]
pub async fn export_debug_bundle(
    destination: String,
    _state: tauri::State<'_, crate::AppState>,
) -> Result<String, String> {
    use std::io::Write;
    use zip::ZipWriter;

    let log_dir = {
        // Find log directory from config
        std::env::var("APPDATA")
            .ok()
            .map(|a| PathBuf::from(a).join("FryEdgeMiner").join("logs"))
    };

    let dest_path = PathBuf::from(&destination);
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let bundle_file = std::fs::File::create(&dest_path).map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(bundle_file);

    // Add log files if found
    if let Some(log_path) = log_dir {
        if log_path.exists() {
            if let Ok(entries) = fs::read_dir(&log_path) {
                for entry in entries.flatten() {
                    if let Ok(metadata) = entry.metadata() {
                        if metadata.is_file() {
                            let file_path = entry.path();
                            if let Ok(contents) = fs::read(&file_path) {
                                // Scrub the contents before adding to zip
                                let contents_str = String::from_utf8_lossy(&contents);
                                let scrubbed = contents_str
                                    .lines()
                                    .map(scrubber::scrub_line)
                                    .collect::<Vec<_>>()
                                    .join("\n");

                                let file_name = file_path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("log");

                                let options: zip::write::FileOptions<()> = Default::default();
                                zip.start_file(file_name, options)
                                    .map_err(|e| e.to_string())?;
                                zip.write_all(scrubbed.as_bytes())
                                    .map_err(|e| e.to_string())?;
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
