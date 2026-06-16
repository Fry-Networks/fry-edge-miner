use super::download::{download_file, partners_base_dir};
use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use tracing::{info, warn};

pub struct SpaceAcresIntegration;

impl SpaceAcresIntegration {
    fn partner_dir() -> PathBuf {
        partners_base_dir().join("space_acres")
    }

    fn binary_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        return Self::partner_dir().join("space-acres.exe");
        #[cfg(not(target_os = "windows"))]
        return Self::partner_dir().join("space-acres");
    }

    async fn fetch_latest_release() -> Result<(String, String)> {
        // Fetch latest release from GitHub API
        let url = "https://api.github.com/repos/autonomys/space-acres/releases/latest";
        let response = reqwest::get(url).await?;
        if !response.status().is_success() {
            anyhow::bail!("Failed to fetch latest release: HTTP {}", response.status());
        }

        let json: serde_json::Value = response.json().await?;
        let tag_name = json["tag_name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No tag_name in release"))?
            .to_string();

        // Find Windows .exe asset
        let assets = json["assets"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("No assets in release"))?;

        let download_url = assets
            .iter()
            .find_map(|asset| {
                if asset["name"].as_str().map(|n| n.ends_with(".exe")).unwrap_or(false) {
                    asset["browser_download_url"].as_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow::anyhow!("No .exe asset found in release"))?;

        Ok((tag_name, download_url))
    }

    fn is_running() -> bool {
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("tasklist")
                .output()
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .to_lowercase()
                        .contains("space-acres.exe")
                })
                .unwrap_or(false)
        }
        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    }
}

#[async_trait]
impl Integration for SpaceAcresIntegration {
    fn id(&self) -> &str {
        "space_acres"
    }

    fn display_name(&self) -> &str {
        "SpaceAcres"
    }

    async fn install(&self) -> Result<()> {
        let binary = Self::binary_path();

        // Check if binary already exists
        if binary.exists() {
            info!(path = ?binary, "SpaceAcres binary already installed");
            return Ok(());
        }

        info!("Installing SpaceAcres from GitHub latest release");

        // Fetch latest release metadata
        let (version, download_url) = Self::fetch_latest_release().await?;
        info!(version = %version, download_url = %download_url, "Found latest release");

        // Download the binary
        download_file(&download_url, &binary).await?;

        // Make it executable on Unix-like systems
        #[cfg(not(target_os = "windows"))]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o755);
            std::fs::set_permissions(&binary, perms)?;
        }

        info!(binary = ?binary, version = %version, "SpaceAcres installed successfully");
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        let binary = Self::binary_path();

        if !binary.exists() {
            anyhow::bail!("SpaceAcres binary not found at {:?}", binary);
        }

        info!(binary = ?binary, "Starting SpaceAcres");

        // Spawn the process with a base directory argument
        let base_dir = Self::partner_dir().join("data");
        std::fs::create_dir_all(&base_dir)?;

        let _ = std::process::Command::new(&binary)
            .arg("--base-directory")
            .arg(&base_dir)
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!("Failed to start SpaceAcres: {}", e)
            })?;

        // Give it a moment to start
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        // Kill any running space-acres process
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/IM", "space-acres.exe", "/F"])
                .output();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = std::process::Command::new("killall")
                .arg("space-acres")
                .output();
        }

        info!("Stopped SpaceAcres");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        let binary = Self::binary_path();
        if !binary.exists() {
            return HealthStatus::Stopped;
        }
        if !has_ssd() {
            warn!("No SSD detected — SpaceAcres performance will be degraded");
            return HealthStatus::Unhealthy(
                "No SSD detected — SpaceAcres performance degraded".to_string(),
            );
        }
        if Self::is_running() {
            HealthStatus::Healthy
        } else {
            HealthStatus::Stopped
        }
    }

    async fn check_update(&self) -> Result<Option<String>> {
        match Self::fetch_latest_release().await {
            Ok((version, _)) => {
                info!(version = %version, "Found SpaceAcres update available");
                Ok(Some(version))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to check SpaceAcres updates");
                Ok(None)
            }
        }
    }

    async fn apply_update(&self, version: &str) -> Result<()> {
        info!(version = %version, "Applying SpaceAcres update");
        // Stop the current instance
        self.stop().await?;
        // Backup old binary
        let binary = Self::binary_path();
        if binary.exists() {
            let backup = binary.with_extension("exe.bak");
            std::fs::copy(&binary, &backup)?;
        }
        // Re-run install which will download the latest
        self.install().await?;
        Ok(())
    }

    fn collect_poc_data(&self) -> PocGateData {
        PocGateData {
            poa: Self::is_running(),
            ..Default::default()
        }
    }
}

/// Detect if system has an SSD
#[cfg(target_os = "windows")]
fn has_ssd() -> bool {
    use std::process::Command;
    Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-PhysicalDisk | Where MediaType -eq 'SSD' | Measure-Object | Select -Expand Count",
        ])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<u32>()
                .unwrap_or(0)
                > 0
        })
        .unwrap_or(false)
}

#[cfg(not(target_os = "windows"))]
fn has_ssd() -> bool {
    false // Platform-specific SSD detection deferred
}
