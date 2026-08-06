use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use tracing::{info, warn};

fn deploy_dir() -> PathBuf {
    dirs::data_local_dir()
        .expect("no local data dir")
        .join("FryEdgeMiner")
        .join("filecoin_checker")
}

fn compose_file() -> PathBuf {
    deploy_dir().join("docker-compose.yml")
}

/// Check if Docker is available via a bounded probe.
fn docker_available() -> bool {
    super::docker_manager::docker_cli_probe_bounded() == Some(true)
}

/// Extract last `n` lines of subprocess output for error display.
fn tail_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

pub struct FilecoinCheckerIntegration;

#[async_trait]
impl Integration for FilecoinCheckerIntegration {
    fn id(&self) -> &str {
        "filecoin_checker"
    }

    fn display_name(&self) -> &str {
        "Filecoin / Checker"
    }

    async fn install(&self) -> Result<()> {
        // Ensure Docker is available, auto-installing if needed
        super::docker_manager::ensure_docker().await?;

        let deploy_dir_path = deploy_dir();
        tokio::fs::create_dir_all(&deploy_dir_path).await?;

        // Write docker-compose.yml from embedded content
        tokio::fs::write(
            compose_file(),
            include_str!("filecoin_checker_deploy/docker-compose.yml"),
        )
        .await?;

        // Pull latest image
        info!("Pulling Filecoin Checker image");
        let output = crate::supervisor::platform::command("docker")
            .args(["compose", "-f", &compose_file().to_string_lossy(), "pull"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(stderr = %stderr, "docker compose pull failed");
            anyhow::bail!("Failed to pull Filecoin Checker image: {}", tail_lines(&stderr, 10));
        }

        info!("Filecoin Checker install complete");
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        super::docker_manager::ensure_docker().await?;
        let compose = compose_file();
        if !compose.exists() {
            anyhow::bail!(
                "Filecoin Checker is not installed yet (deploy directory missing at {}) — toggle it off and on to reinstall",
                deploy_dir().display()
            );
        }

        // install() only runs when installed_version() is None, so an existing
        // deploy dir would otherwise pin a stale compose file forever. Refresh
        // it from the shipped content on every start.
        tokio::fs::write(
            &compose,
            include_str!("filecoin_checker_deploy/docker-compose.yml"),
        )
        .await?;

        info!("Starting Filecoin Checker containers");
        let output = crate::supervisor::platform::command("docker")
            .args(["compose", "-f", &compose.to_string_lossy(), "up", "-d"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(stderr = %stderr, "Failed to start Filecoin Checker");
            anyhow::bail!("Failed to start Filecoin Checker: {}", tail_lines(&stderr, 10));
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        let compose = compose_file();
        if compose.exists() {
            crate::supervisor::platform::command("docker")
                .args(["compose", "-f", &compose.to_string_lossy(), "stop"])
                .output()?;
            info!("Stopped Filecoin Checker containers");
        }
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        if !docker_available() {
            return HealthStatus::Unhealthy("Docker not available".to_string());
        }

        let compose = compose_file();
        if !compose.exists() {
            return HealthStatus::Stopped;
        }

        // Check if container is running via docker ps
        let output = match crate::supervisor::platform::command("docker")
            .args(["ps", "--filter", "label=com.docker.compose.project=filecoin_checker"])
            .output()
        {
            Ok(o) => o,
            Err(_) => return HealthStatus::Unhealthy("Failed to query Docker".to_string()),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let is_running = stdout.lines().count() > 1; // header + at least one container line

        if !is_running {
            return HealthStatus::Stopped;
        }

        // Container is running — check logs for health markers
        let logs_output = match crate::supervisor::platform::command("docker")
            .args(["compose", "-f", &compose.to_string_lossy(), "logs", "--tail", "50"])
            .output()
        {
            Ok(o) => o,
            // Container is confirmed running but logs are unreadable — health is
            // unverified, so report Starting (poa stays false) rather than Healthy.
            Err(_) => return HealthStatus::Starting,
        };

        let logs = format!(
            "{}{}",
            String::from_utf8_lossy(&logs_output.stdout),
            String::from_utf8_lossy(&logs_output.stderr)
        );

        // The Checker validates its reward wallet by calling
        // station-wallet-screening.fly.dev, which no longer resolves (the
        // service was decommissioned and the upstream repo is unmaintained).
        // Every published version still calls it, so this is not something a
        // funded wallet or a config change can fix.
        if logs.contains("Failed to validate FIL_WALLET_ADDRESS") {
            return HealthStatus::Unhealthy(
                "Checker cannot start — its upstream wallet-screening service \
                 (station-wallet-screening.fly.dev) has been decommissioned, so wallet \
                 validation can never succeed. Waiting on an upstream fix."
                    .to_string(),
            );
        }

        // Check for error or exit markers
        if logs.contains("error") || logs.contains("ERROR") || logs.contains("panic") {
            return HealthStatus::Unhealthy("Error detected in Checker logs".to_string());
        }

        // Check for positive activity markers
        if logs.contains("jobs-completed") || logs.contains("activity:info") {
            return HealthStatus::Healthy;
        }

        // Container up but not enough confirmation yet
        HealthStatus::Starting
    }

    async fn check_update(&self) -> Result<Option<String>> {
        Ok(None) // Docker images auto-update via pull
    }

    async fn apply_update(&self, _version: &str) -> Result<()> {
        self.install().await // re-pull latest images
    }

    fn installed_version(&self) -> Option<String> {
        if compose_file().exists() {
            Some("22.3.1".to_string())
        } else {
            None
        }
    }

    fn collect_poc_data(&self) -> PocGateData {
        let compose_exists = compose_file().exists();
        PocGateData {
            poa: compose_exists && docker_available(),
            ..Default::default()
        }
    }

    fn requires_docker(&self) -> bool {
        true
    }
}
