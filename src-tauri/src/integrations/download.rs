use std::path::{Path, PathBuf};
use tracing::info;

/// Get the base directory for FEM partner binaries
pub fn partners_base_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("C:/ProgramData"))
        .join("FryEdgeMiner")
        .join("partners")
}

/// Download a file from URL to destination path
pub async fn download_file(url: &str, dest: &Path) -> anyhow::Result<()> {
    info!(url = url, dest = ?dest, "Downloading file");
    let response = reqwest::get(url).await?;
    if !response.status().is_success() {
        anyhow::bail!("Download failed: HTTP {}", response.status());
    }
    let bytes = response.bytes().await?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(dest, &bytes)?;
    info!(dest = ?dest, bytes = bytes.len(), "Download complete");
    Ok(())
}
