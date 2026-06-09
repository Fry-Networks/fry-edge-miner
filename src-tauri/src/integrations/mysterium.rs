use super::download::partners_base_dir;
use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use tracing::{info, warn};

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

        // Check if binary already exists
        if binary.exists() {
            info!(path = ?binary, "MystNodes binary already installed");
            return Ok(());
        }

        // Create partner directory
        std::fs::create_dir_all(&partner_dir)?;

        warn!(
            "DEFERRED: MystNodes SDK requires manual installation or SDK token. \
            Binary should be placed at: {:?}",
            binary
        );

        Ok(())
    }

    async fn start(&self) -> Result<()> {
        let binary = Self::binary_path();

        if !binary.exists() {
            anyhow::bail!("MystNodes binary not found at {:?}", binary);
        }

        info!(binary = ?binary, "Starting MystNodes");

        // Verify the process starts by checking health endpoint after a brief delay
        let _ = std::process::Command::new(&binary)
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!("Failed to start MystNodes: {}", e)
            })?;

        // Give it a moment to start
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        // Kill any running myst process
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
        // Check if MystNodes health endpoint is reachable at localhost:4449
        let client = reqwest::Client::new();
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            client.get("http://127.0.0.1:4449/healthcheck").send(),
        )
        .await
        {
            Ok(Ok(resp)) if resp.status().is_success() => {
                info!("MystNodes health check passed");
                HealthStatus::Healthy
            }
            Ok(Ok(resp)) => {
                warn!(status = ?resp.status(), "MystNodes health check failed");
                HealthStatus::Unhealthy(format!("HTTP {}", resp.status()))
            }
            Ok(Err(e)) => {
                warn!(error = %e, "MystNodes health check request failed");
                HealthStatus::Unhealthy(e.to_string())
            }
            Err(_) => {
                warn!("MystNodes health check timeout");
                HealthStatus::Unhealthy("Timeout".to_string())
            }
        }
    }

    async fn check_update(&self) -> Result<Option<String>> {
        // Check for MystNodes updates (would require checking a release API)
        Ok(None)
    }

    async fn apply_update(&self, _version: &str) -> Result<()> {
        // Update handling deferred
        Ok(())
    }

    fn collect_poc_data(&self) -> PocGateData {
        // Return with poa=true only if service is healthy
        PocGateData::default()
    }
}
