use super::download::{download_file, partners_base_dir};
use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use tracing::{info, warn};

const GITHUB_RELEASES_API: &str =
    "https://api.github.com/repos/mysteriumnetwork/node/releases/latest";
const WINDOWS_ASSET_NAME: &str = "myst_windows_amd64.zip";

pub struct MysteriumIntegration;

impl MysteriumIntegration {
    fn partner_dir() -> PathBuf {
        partners_base_dir().join("mysterium")
    }

    fn binary_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        return Self::partner_dir().join("myst.exe");
        #[cfg(not(target_os = "windows"))]
        return Self::partner_dir().join("myst");
    }

    /// Fetch latest release download URL from GitHub
    async fn fetch_latest_release() -> Result<(String, String)> {
        let client = reqwest::Client::builder()
            .user_agent("FEM/0.2")
            .build()?;
        let response = client.get(GITHUB_RELEASES_API).send().await?;
        if !response.status().is_success() {
            anyhow::bail!("Failed to fetch release: HTTP {}", response.status());
        }
        let json: serde_json::Value = response.json().await?;
        let tag = json["tag_name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No tag_name"))?
            .to_string();
        let assets = json["assets"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("No assets"))?;
        let url = assets
            .iter()
            .find_map(|a| {
                if a["name"].as_str() == Some(WINDOWS_ASSET_NAME) {
                    a["browser_download_url"].as_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow::anyhow!("No {} asset found", WINDOWS_ASSET_NAME))?;
        Ok((tag, url))
    }

    /// Extract myst binary from downloaded zip
    fn extract_zip(zip_path: &std::path::Path, dest_dir: &std::path::Path) -> Result<()> {
        let file = std::fs::File::open(zip_path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();
            if name.contains("myst") && (name.ends_with(".exe") || !name.contains('.')) {
                let dest = dest_dir.join(
                    std::path::Path::new(&name)
                        .file_name()
                        .unwrap_or_else(|| std::ffi::OsStr::new("myst")),
                );
                let mut out = std::fs::File::create(&dest)?;
                std::io::copy(&mut entry, &mut out)?;
                info!(extracted = ?dest, "Extracted MystNodes binary");
                return Ok(());
            }
        }
        anyhow::bail!("No myst binary found in zip archive")
    }
}

#[async_trait]
impl Integration for MysteriumIntegration {
    fn id(&self) -> &str {
        "mysterium"
    }

    fn display_name(&self) -> &str {
        "MystNodes"
    }

    async fn install(&self) -> Result<()> {
        let partner_dir = Self::partner_dir();
        let binary = Self::binary_path();

        if binary.exists() {
            info!(path = ?binary, "MystNodes binary already installed");
            return Ok(());
        }

        std::fs::create_dir_all(&partner_dir)?;

        info!("Installing MystNodes from GitHub release");
        let (version, download_url) = Self::fetch_latest_release().await?;
        info!(version = %version, "Found MystNodes release");

        let zip_path = partner_dir.join(WINDOWS_ASSET_NAME);
        download_file(&download_url, &zip_path).await?;

        Self::extract_zip(&zip_path, &partner_dir)?;
        std::fs::remove_file(&zip_path).ok(); // cleanup zip

        info!(version = %version, "MystNodes installed");
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        let binary = Self::binary_path();
        if !binary.exists() {
            anyhow::bail!("MystNodes binary not found at {:?}", binary);
        }
        info!(binary = ?binary, "Starting MystNodes");
        let _ = std::process::Command::new(&binary)
            .arg("service")
            .arg("--agreed-terms-and-conditions")
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to start MystNodes: {}", e))?;
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("taskkill")
                .args(&["/IM", "myst.exe", "/F"])
                .output();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = std::process::Command::new("killall")
                .arg("myst")
                .output();
        }
        info!("Stopped MystNodes");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        let client = reqwest::Client::new();
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            client.get("http://127.0.0.1:4449/healthcheck").send(),
        )
        .await
        {
            Ok(Ok(resp)) if resp.status().is_success() => HealthStatus::Healthy,
            Ok(Ok(resp)) => HealthStatus::Unhealthy(format!("HTTP {}", resp.status())),
            Ok(Err(e)) => HealthStatus::Unhealthy(e.to_string()),
            Err(_) => HealthStatus::Unhealthy("Timeout".to_string()),
        }
    }

    async fn check_update(&self) -> Result<Option<String>> {
        match Self::fetch_latest_release().await {
            Ok((version, _)) => Ok(Some(version)),
            Err(e) => {
                warn!(error = %e, "Failed to check MystNodes updates");
                Ok(None)
            }
        }
    }

    async fn apply_update(&self, _version: &str) -> Result<()> {
        self.stop().await?;
        let binary = Self::binary_path();
        if binary.exists() {
            std::fs::rename(&binary, binary.with_extension("exe.bak"))?;
        }
        self.install().await
    }

    fn collect_poc_data(&self) -> PocGateData {
        PocGateData::default()
    }
}
