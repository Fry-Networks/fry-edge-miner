use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use tracing::{info, warn};

const CONTAINER_NAME: &str = "presearch-node";
const DOCKER_IMAGE: &str = "presearch/node:latest";
const PRESEARCH_REG_CODE: Option<&str> = option_env!("PRESEARCH_REG_CODE");

pub struct PresearchIntegration;

fn docker_available() -> bool {
    std::process::Command::new("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn container_exists() -> bool {
    std::process::Command::new("docker")
        .args(["ps", "-a", "--filter", &format!("name={}", CONTAINER_NAME), "--format", "{{.Names}}"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(CONTAINER_NAME))
        .unwrap_or(false)
}

fn container_running() -> bool {
    std::process::Command::new("docker")
        .args(["inspect", CONTAINER_NAME, "--format", "{{.State.Running}}"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false)
}

#[async_trait]
impl Integration for PresearchIntegration {
    fn id(&self) -> &str {
        "presearch"
    }

    fn display_name(&self) -> &str {
        "Presearch"
    }

    async fn install(&self) -> Result<()> {
        if !docker_available() {
            warn!("Docker not available — Presearch requires Docker");
            return Ok(());
        }
        info!("Pulling Presearch node image");
        let output = std::process::Command::new("docker")
            .args(["pull", DOCKER_IMAGE])
            .output()?;
        if !output.status.success() {
            warn!(
                stderr = %String::from_utf8_lossy(&output.stderr),
                "Failed to pull Presearch image"
            );
        }
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        if !docker_available() {
            anyhow::bail!("Docker not available");
        }
        if container_running() {
            info!("Presearch container already running");
            return Ok(());
        }
        if container_exists() {
            info!("Starting existing Presearch container");
            std::process::Command::new("docker")
                .args(["start", CONTAINER_NAME])
                .output()?;
        } else {
            info!("Creating new Presearch container");
            let mut args: Vec<String> = vec![
                "run".into(), "-d".into(), "--name".into(), CONTAINER_NAME.into(),
                "-v".into(), "presearch-node-storage:/app/node".into(),
            ];
            // .filter avoids "-e REGISTRATION_CODE=" when GitHub secret is empty
            if let Some(code) = PRESEARCH_REG_CODE.filter(|s| !s.is_empty()) {
                let reg_env = format!("REGISTRATION_CODE={}", code);
                args.extend_from_slice(&["-e".into(), reg_env]);
            } else {
                warn!("No PRESEARCH_REG_CODE set — starting Presearch in unclaimed mode");
            }
            args.push(DOCKER_IMAGE.into());
            std::process::Command::new("docker")
                .args(&args)
                .output()?;
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        if container_running() {
            std::process::Command::new("docker")
                .args(["stop", CONTAINER_NAME])
                .output()?;
            info!("Stopped Presearch container");
        }
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        if !docker_available() {
            return HealthStatus::Unhealthy("Docker not available".to_string());
        }
        if container_running() {
            HealthStatus::Healthy
        } else if container_exists() {
            HealthStatus::Stopped
        } else {
            HealthStatus::Stopped
        }
    }

    async fn check_update(&self) -> Result<Option<String>> {
        Ok(None) // Docker image auto-updates via pull
    }

    async fn apply_update(&self, _version: &str) -> Result<()> {
        self.install().await // re-pull latest image
    }

    fn collect_poc_data(&self) -> PocGateData {
        PocGateData {
            poa: container_running(),
            ..Default::default()
        }
    }
}
