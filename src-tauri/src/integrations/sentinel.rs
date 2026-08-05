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

impl SentinelIntegration {
    /// Public address the node advertises. Falls back to the image default
    /// (127.0.0.1) when lookup fails — the node still boots, it is simply not
    /// reachable from the network until the operator forwards a port.
    async fn public_addr() -> String {
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
        {
            Ok(c) => c,
            Err(_) => return "127.0.0.1".to_string(),
        };
        match client.get("https://api.ipify.org").send().await {
            Ok(r) => r
                .text()
                .await
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && s.len() < 64)
                .unwrap_or_else(|| "127.0.0.1".to_string()),
            Err(_) => "127.0.0.1".to_string(),
        }
    }

    fn compose_run(compose: &std::path::Path, extra: &[&str], stdin_lines: Option<&str>) -> Result<std::process::Output> {
        let compose_str = compose.to_string_lossy().to_string();
        let mut args: Vec<&str> = vec!["compose", "-f", &compose_str, "run", "--rm", "--no-deps"];
        args.push("sentinel-dvpnx");
        args.extend_from_slice(extra);

        let mut child = crate::supervisor::platform::command("docker")
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        if let Some(input) = stdin_lines {
            use std::io::Write;
            if let Some(mut sin) = child.stdin.take() {
                let _ = sin.write_all(input.as_bytes());
            }
        } else {
            drop(child.stdin.take());
        }
        Ok(child.wait_with_output()?)
    }

    /// Idempotent: `init` aborts if config exists, `keys add` fails if the key
    /// exists — both are treated as "already provisioned".
    async fn init_node_config(compose: &std::path::Path) -> Result<()> {
        let addr = Self::public_addr().await;
        info!(remote_addr = %addr, "Initializing Sentinel node config");

        let out = Self::compose_run(
            compose,
            &[
                "init",
                "--keyring.backend",
                "test",
                "--node.service-type",
                "wireguard",
                "--node.remote-addrs",
                &addr,
            ],
            None,
        )?;
        let init_err = String::from_utf8_lossy(&out.stderr).to_string();
        if !out.status.success() && !init_err.contains("already exists") {
            warn!(stderr = %tail_lines(&init_err, 5), "Sentinel init reported an error");
        }

        // Empty mnemonic line → generate a fresh key; empty passphrase line.
        let keys = Self::compose_run(
            compose,
            &["keys", "add", "main", "--keyring.backend", "test"],
            Some("\n\n"),
        )?;
        let keys_out = format!(
            "{}{}",
            String::from_utf8_lossy(&keys.stdout),
            String::from_utf8_lossy(&keys.stderr)
        );
        if keys.status.success() {
            if let Some(line) = keys_out.lines().find(|l| l.trim_start().starts_with("address:")) {
                info!(node_account = %line.trim(), "Sentinel node key created");
            }
        } else if !keys_out.contains("already exists") {
            warn!(detail = %tail_lines(&keys_out, 5), "Sentinel key creation reported an error");
        }
        Ok(())
    }

    /// Node account address, read back from the keyring for the fund-me hint.
    fn node_address(compose: &std::path::Path) -> Option<String> {
        let out = Self::compose_run(
            compose,
            &["keys", "show", "main", "--keyring.backend", "test"],
            None,
        )
        .ok()?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        text.lines()
            .find(|l| l.trim_start().starts_with("address:"))
            .and_then(|l| l.split(':').nth(1))
            .map(|s| s.trim().to_string())
            .filter(|s| s.starts_with("sent1"))
    }
}

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

        // First run: generate config.toml + a per-device `main` key inside the
        // named volume. `dvpnx start` refuses to boot without both.
        Self::init_node_config(&compose_file()).await?;

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
                            let logs = format!(
                                "{}{}",
                                String::from_utf8_lossy(&log_output.stdout),
                                String::from_utf8_lossy(&log_output.stderr)
                            );
                            // Sentinel Hub refuses to start a node whose account
                            // has never received DVPN — surface the address to
                            // fund instead of a raw stack line.
                            if logs.contains("does not exist") && logs.contains("account") {
                                let addr = Self::node_address(&compose)
                                    .unwrap_or_else(|| "the node account".to_string());
                                return HealthStatus::Unhealthy(format!(
                                    "Sentinel node account not funded — send DVPN to {} to activate this node",
                                    addr
                                ));
                            }
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
