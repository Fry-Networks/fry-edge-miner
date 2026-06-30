use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};
use crate::api::client::ApiClient;
use crate::config::store::ConfigStore;

const HEALTH_URL: &str = "http://localhost:8181/health";
/// Diiisco bearer token: runtime env var → compile-time option_env! → empty default.
fn diiisco_bearer_token() -> String {
    std::env::var("DIIISCO_BEARER_TOKEN")
        .ok()
        .or_else(|| option_env!("DIIISCO_BEARER_TOKEN").map(|s| s.to_string()))
        .unwrap_or_default()
}

pub struct DiiiscoIntegration {
    pub api_client: Arc<ApiClient>,
    pub config: Arc<ConfigStore>,
}

fn deploy_dir() -> PathBuf {
    dirs::data_local_dir()
        .expect("no local data dir")
        .join("FryEdgeMiner")
        .join("diiisco")
}

fn docker_available() -> bool {
    crate::supervisor::platform::command("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn compose_file() -> PathBuf {
    deploy_dir().join("docker-compose.yml")
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
            return Err(anyhow::anyhow!("Docker not available — Diiisco requires Docker"));
        }

        let deploy_dir = dirs::data_local_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot resolve local app data dir"))?
            .join("FryEdgeMiner")
            .join("diiisco");

        // Write Docker files from embedded content
        let node_dir = deploy_dir.join("diiisco-node");
        tokio::fs::create_dir_all(&node_dir).await?;
        tokio::fs::write(
            deploy_dir.join("docker-compose.yml"),
            include_str!("diiisco_deploy/docker-compose.yml"),
        )
        .await?;
        tokio::fs::write(
            node_dir.join("Dockerfile"),
            include_str!("diiisco_deploy/diiisco-node/Dockerfile"),
        )
        .await?;

        // Fetch credentials from hardwareapi
        let cfg = self.config.get();
        let miner_key = cfg.miner_key.as_deref().ok_or_else(|| {
            anyhow::anyhow!("Miner key not set — complete device registration before installing Diiisco")
        })?;
        let creds = crate::api::credentials::lookup(&self.api_client, miner_key).await
            .map_err(|e| anyhow::anyhow!("Failed to fetch credentials: {}", e))?;
        let algo_address = creds.algo_address.ok_or_else(|| {
            anyhow::anyhow!("Device has no Algorand wallet — re-register or contact support")
        })?;
        let algo_mnemonic = creds.algo_mnemonic.ok_or_else(|| {
            anyhow::anyhow!("Device Algorand mnemonic unavailable — contact support")
        })?;

        // Docker compose build with credentials as env vars (NOT command-line args)
        let bearer = diiisco_bearer_token();
        info!("Building Diiisco Docker image");
        let output = crate::supervisor::platform::command("docker")
            .args(["compose", "build"])
            .env("ALGO_ADDRESS", &algo_address)
            .env("ALGO_MNEMONIC", &algo_mnemonic)
            .env("DIIISCO_API_KEY", bearer)
            .current_dir(&deploy_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("docker compose build failed: {}", stderr);
        }
        info!("Diiisco install complete — image built with per-device wallet");
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        if !docker_available() {
            anyhow::bail!("Docker not available");
        }
        let compose = compose_file();
        if !compose.exists() {
            anyhow::bail!("Diiisco deploy directory not found at {}", deploy_dir().display());
        }
        info!("Starting Diiisco containers");
        let output = crate::supervisor::platform::command("docker")
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
            crate::supervisor::platform::command("docker")
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
        let token = diiisco_bearer_token();
        if token.is_empty() {
            warn!("No DIIISCO_BEARER_TOKEN set — health check may fail");
        }
        let client = reqwest::Client::new();
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            client
                .get(HEALTH_URL)
                .bearer_auth(token)
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

    fn installed_version(&self) -> Option<String> {
        if compose_file().exists() {
            Some("installed".into())
        } else {
            None
        }
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
