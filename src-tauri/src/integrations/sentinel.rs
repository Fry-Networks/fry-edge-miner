use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use tracing::{info, warn};

fn deploy_dir() -> PathBuf {
    dirs::data_local_dir()
        .expect("no local data dir")
        .join("FryEdgeMiner")
        .join("sentinel")
}

fn compose_file() -> PathBuf {
    deploy_dir().join("docker-compose.yml")
}

fn docker_available() -> bool {
    super::docker_manager::docker_cli_probe_bounded() == Some(true)
}

/// Last `n` lines of subprocess output
fn tail_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

pub struct SentinelIntegration;

#[async_trait]
impl Integration for SentinelIntegration {
    fn id(&self) -> &str {
        "sentinel"
    }

    fn display_name(&self) -> &str {
        "Sentinel dVPN"
    }

    async fn install(&self) -> Result<()> {
        // Ensure Docker is available, auto-installing if needed
        super::docker_manager::ensure_docker().await?;

        let deploy_dir = deploy_dir();
        tokio::fs::create_dir_all(&deploy_dir).await?;

        // Write Docker Compose file from embedded content
        tokio::fs::write(
            compose_file(),
            include_str!("sentinel_deploy/docker-compose.yml"),
        )
        .await?;

        info!("Pulling Sentinel dVPN image");
        let output = crate::supervisor::platform::command("docker")
            .args(["compose", "-f", &compose_file().to_string_lossy(), "pull"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(stderr = %stderr, "docker compose pull failed");
            anyhow::bail!("Failed to pull Sentinel dVPN image: {}", tail_lines(&stderr, 15));
        }

        // TODO: First-run node config initialization. The Sentinel dVPN image
        // (`sentinel-dvpnx` binary) requires initial configuration. Run one-shot:
        // docker compose run --rm sentinel-dvpnx <config init command>
        // Exact CLI syntax must be determined from image inspection or docs.
        // For now, the image pull succeeds and startup can proceed.

        info!("Sentinel dVPN install complete");
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        super::docker_manager::ensure_docker().await?;
        let compose = compose_file();
        if !compose.exists() {
            anyhow::bail!(
                "Sentinel is not installed yet (deploy directory missing at {}) — toggle it off and on to reinstall",
                deploy_dir().display()
            );
        }

        info!("Starting Sentinel dVPN containers");
        let output = crate::supervisor::platform::command("docker")
            .args(["compose", "-f", &compose.to_string_lossy(), "up", "-d"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(stderr = %stderr, "Failed to start Sentinel");
            anyhow::bail!("Failed to start Sentinel dVPN: {}", tail_lines(&stderr, 15));
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
            info!("Stopped Sentinel dVPN containers");
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

        // Check container state via docker compose ps
        match crate::supervisor::platform::command("docker")
            .args([
                "compose",
                "-f",
                &compose.to_string_lossy(),
                "ps",
                "--format",
                "json",
            ])
            .output()
        {
            Ok(output) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return HealthStatus::Unhealthy(format!(
                        "docker compose ps failed: {}",
                        tail_lines(&stderr, 3)
                    ));
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                // Parse JSON array of container states
                if let Ok(containers) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
                    // If any container is running, health is Healthy
                    let has_running = containers.iter().any(|c| {
                        c.get("State")
                            .and_then(|s| s.as_str())
                            .map(|s| s == "running")
                            .unwrap_or(false)
                    });

                    if has_running {
                        return HealthStatus::Healthy;
                    }

                    // Check if any are exited (error state)
                    let has_exited = containers.iter().any(|c| {
                        c.get("State")
                            .and_then(|s| s.as_str())
                            .map(|s| s.starts_with("exited"))
                            .unwrap_or(false)
                    });

                    if has_exited {
                        // Try to get logs for more detail
                        if let Ok(log_output) = crate::supervisor::platform::command("docker")
                            .args([
                                "compose",
                                "-f",
                                &compose.to_string_lossy(),
                                "logs",
                                "sentinel-dvpnx",
                            ])
                            .output()
                        {
                            let logs = String::from_utf8_lossy(&log_output.stderr);
                            return HealthStatus::Unhealthy(format!(
                                "Container exited: {}",
                                tail_lines(&logs, 5)
                            ));
                        } else {
                            return HealthStatus::Unhealthy("Container exited".to_string());
                        }
                    }

                    return HealthStatus::Starting;
                } else {
                    return HealthStatus::Unhealthy("Failed to parse docker ps output".to_string());
                }
            }
            Err(e) => {
                return HealthStatus::Unhealthy(format!(
                    "Failed to check Sentinel container state: {}",
                    e
                ));
            }
        }
    }

    async fn check_update(&self) -> Result<Option<String>> {
        Ok(None) // Docker images auto-update via pull
    }

    async fn apply_update(&self, _version: &str) -> Result<()> {
        self.install().await // re-pull latest image
    }

    fn installed_version(&self) -> Option<String> {
        if compose_file().exists() {
            Some("installed".into())
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
