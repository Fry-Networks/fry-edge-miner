use super::{HealthStatus, Integration, PocGateData};
use crate::config::store::ConfigStore;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{info, warn};

const LEGACY_CONTAINER_NAME: &str = "presearch-node";
const DOCKER_IMAGE: &str = "presearch/node:latest";
const PRESEARCH_REG_CODE: Option<&str> = option_env!("PRESEARCH_REG_CODE");

pub struct PresearchIntegration {
    pub config: Arc<ConfigStore>,
}

impl PresearchIntegration {
    fn suffix(&self) -> Option<String> {
        self.config.get().miner_key.as_ref().map(|key| {
            let lower = key.to_lowercase();
            lower[lower.len().saturating_sub(8)..].to_string()
        })
    }

    fn container_name(&self) -> String {
        match self.suffix() {
            Some(suffix) => format!("presearch-{}", suffix),
            None => "presearch-node-unknown".to_string(),
        }
    }

    fn docker_available(&self) -> bool {
        crate::supervisor::platform::command("docker")
            .arg("info")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn container_exists(&self, name: &str) -> bool {
        crate::supervisor::platform::command("docker")
            .args(["ps", "-a", "--filter", &format!("name={}", name), "--format", "{{.Names}}"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(name))
            .unwrap_or(false)
    }

    fn image_pulled(&self) -> bool {
        crate::supervisor::platform::command("docker")
            .args(["images", "-q", DOCKER_IMAGE])
            .output()
            .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false)
    }

    fn container_running(&self, name: &str) -> bool {
        crate::supervisor::platform::command("docker")
            .args(["inspect", name, "--format", "{{.State.Running}}"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
            .unwrap_or(false)
    }

    fn remove_container(&self, name: &str) {
        let _ = crate::supervisor::platform::command("docker")
            .args(["rm", "-f", name])
            .output();
    }
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
        if !self.docker_available() {
            warn!("Docker not available — Presearch requires Docker");
            return Ok(());
        }
        info!("Pulling Presearch node image");
        let output = crate::supervisor::platform::command("docker")
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
        if !self.docker_available() {
            anyhow::bail!("Docker not available");
        }

        let target = self.container_name();
        let suffix = self.suffix().unwrap_or_else(|| "unknown".to_string());
        info!(target = %target, suffix = %suffix, "Presearch container target");

        // Migration: if a legacy presearch-node container exists and the keyed target does not,
        // stop and remove the legacy container so we can recreate under the new keyed name.
        let legacy_exists = self.container_exists(LEGACY_CONTAINER_NAME);
        let target_exists = self.container_exists(&target);
        let target_running = self.container_running(&target);

        if target_running {
            warn!(target = %target, "Presearch target container already running; skipping legacy migration");
            return Ok(());
        }

        if target_exists {
            info!(target = %target, "Starting existing keyed Presearch container");
            crate::supervisor::platform::command("docker")
                .args(["start", &target])
                .output()?;
        } else if legacy_exists {
            info!("Migrating legacy Presearch container to keyed name");
            let _ = crate::supervisor::platform::command("docker")
                .args(["stop", LEGACY_CONTAINER_NAME])
                .output();
            self.remove_container(LEGACY_CONTAINER_NAME);
            self.create_container(&target, &suffix).await?;
        } else {
            info!("Creating new Presearch container");
            self.create_container(&target, &suffix).await?;
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        let target = self.container_name();
        if self.container_running(&target) {
            crate::supervisor::platform::command("docker")
                .args(["stop", &target])
                .output()?;
            info!(target = %target, "Stopped Presearch container");
        }
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        if !self.docker_available() {
            return HealthStatus::Unhealthy("Docker not available".to_string());
        }
        let target = self.container_name();
        if self.container_running(&target) {
            HealthStatus::Healthy
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

    fn installed_version(&self) -> Option<String> {
        if !self.docker_available() {
            return None;
        }
        let target = self.container_name();
        if self.container_exists(&target)
            || self.container_exists(LEGACY_CONTAINER_NAME)
            || self.image_pulled()
        {
            Some("installed".into())
        } else {
            None
        }
    }

    fn collect_poc_data(&self) -> PocGateData {
        let target = self.container_name();
        PocGateData {
            poa: self.container_running(&target),
            ..Default::default()
        }
    }
}

impl PresearchIntegration {
    async fn create_container(&self, name: &str, suffix: &str) -> Result<()> {
        let mut args: Vec<String> = vec![
            "run".into(),
            "-d".into(),
            "--name".into(),
            name.into(),
            "--hostname".into(),
            suffix.into(),
            "-v".into(),
            "presearch-node-storage:/app/node".into(),
        ];
        if let Some(code) = PRESEARCH_REG_CODE.filter(|s| !s.is_empty()) {
            let reg_env = format!("REGISTRATION_CODE={}", code);
            args.extend_from_slice(&["-e".into(), reg_env]);
        } else {
            warn!("No PRESEARCH_REG_CODE set — starting Presearch in unclaimed mode");
        }
        args.push(DOCKER_IMAGE.into());
        crate::supervisor::platform::command("docker")
            .args(&args)
            .output()?;
        Ok(())
    }
}
