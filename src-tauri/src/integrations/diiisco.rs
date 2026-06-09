use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;
use tracing::{info, warn};

const DEPLOY_DIR: &str = "C:/tmp/diiisco-deploy";
const HEALTH_URL: &str = "http://localhost:8181/health";
const BEARER_TOKEN: &str = "sk-diiisco-prod-key";

pub struct DiiiscoIntegration;

fn docker_available() -> bool {
    std::process::Command::new("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn compose_file() -> std::path::PathBuf {
    Path::new(DEPLOY_DIR).join("docker-compose.yml")
}

#[async_trait]
impl Integration for DiiiscoIntegration {
    fn id(&self) -> &str {
        "diiisco"
    }

    fn display_name(&self) -> &str {
        "Diiisco"
    }

    async fn install(&self) -> Result<()> {
        if !docker_available() {
            warn!("Docker not available — Diiisco requires Docker");
            return Ok(());
        }
        let compose = compose_file();
        if !compose.exists() {
            warn!(
                "DEFERRED: Diiisco deploy directory not found at {}. \
                Requires git clone + ALGO credentials to initialize.",
                DEPLOY_DIR
            );
            return Ok(());
        }
        info!("Pulling Diiisco images");
        let output = std::process::Command::new("docker")
            .args(["compose", "-f", &compose.to_string_lossy(), "pull"])
            .output()?;
        if !output.status.success() {
            warn!(
                stderr = %String::from_utf8_lossy(&output.stderr),
                "Failed to pull Diiisco images"
            );
        }
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        if !docker_available() {
            anyhow::bail!("Docker not available");
        }
        let compose = compose_file();
        if !compose.exists() {
            anyhow::bail!("Diiisco deploy directory not found at {}", DEPLOY_DIR);
        }
        info!("Starting Diiisco containers");
        let output = std::process::Command::new("docker")
            .args(["compose", "-f", &compose.to_string_lossy(), "up", "-d"])
            .output()?;
        if !output.status.success() {
            anyhow::bail!(
                "Failed to start Diiisco: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        let compose = compose_file();
        if compose.exists() {
            std::process::Command::new("docker")
                .args(["compose", "-f", &compose.to_string_lossy(), "stop"])
                .output()?;
            info!("Stopped Diiisco containers");
        }
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        if !docker_available() {
            return HealthStatus::Unhealthy("Docker not available".to_string());
        }
        let client = reqwest::Client::new();
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            client
                .get(HEALTH_URL)
                .bearer_auth(BEARER_TOKEN)
                .send(),
        )
        .await
        {
            Ok(Ok(resp)) if resp.status().is_success() => HealthStatus::Healthy,
            Ok(Ok(resp)) => {
                HealthStatus::Unhealthy(format!("HTTP {}", resp.status()))
            }
            Ok(Err(_)) => HealthStatus::Stopped, // connection refused = not running
            Err(_) => HealthStatus::Unhealthy("Timeout".to_string()),
        }
    }

    async fn check_update(&self) -> Result<Option<String>> {
        Ok(None) // Docker images auto-update via pull
    }

    async fn apply_update(&self, _version: &str) -> Result<()> {
        self.install().await // re-pull latest images
    }

    fn collect_poc_data(&self) -> PocGateData {
        // Synchronous check — can't do async health here, check compose file existence
        let compose_exists = compose_file().exists();
        PocGateData {
            poa: compose_exists && docker_available(),
            ..Default::default()
        }
    }
}
