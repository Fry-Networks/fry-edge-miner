use crate::supervisor::platform::BoundedOutput;
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

/// Verdict derived from the `docker compose ps` container states.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ComposeVerdict {
    Running,
    /// At least one container finished (exited/dead) — fetch logs for detail.
    Exited,
    /// A container exists but cannot progress without intervention.
    Stuck(String),
    /// Containers are transitioning (created/restarting) — genuinely starting.
    Starting,
    /// The compose project has no containers at all.
    NoContainers,
}

pub(crate) fn classify_compose_states(states: &[String]) -> ComposeVerdict {
    if states.iter().any(|s| s == "running") {
        return ComposeVerdict::Running;
    }
    // "dead" is as final as "exited" — both need the log-detail path, and
    // both must NOT report Starting (the v0.4.7 stuck-STARTING bug: a
    // stopped container never became Unhealthy, so the supervisor never
    // restarted it).
    if states.iter().any(|s| s.starts_with("exited") || s == "dead") {
        return ComposeVerdict::Exited;
    }
    if states.is_empty() {
        return ComposeVerdict::NoContainers;
    }
    if let Some(stuck) = states.iter().find(|s| *s == "paused" || *s == "removing") {
        return ComposeVerdict::Stuck(stuck.clone());
    }
    ComposeVerdict::Starting
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

    /// F16: minimum node balance (in udvpn) below which a Sentinel node cannot
    /// register on chain. Each node runs its own Fry-managed wallet.
    const MIN_FUNDING_UDVPN: u64 = 10_000_000; // 10 DVPN (6 decimals)

    /// F16 gate: whether FEM should actually transfer DVPN from the Fry treasury
    /// to under-funded nodes. Default OFF — the per-node wallet management and
    /// the funding decision below both run, but no live transfer happens unless
    /// this is explicitly enabled. Keeps the capability shipped without moving
    /// real funds.
    fn auto_fund_enabled() -> bool {
        std::env::var("FRY_SENTINEL_AUTO_FUND")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    }

    /// F16: keep each node's Fry-managed wallet funded so it can register on
    /// chain. The per-node key is generated and held inside the container — the
    /// user has no access to it. This resolves the node address and decides
    /// whether it needs funding; the actual treasury transfer is gated OFF by
    /// default, so no live DVPN moves. When auto-funding is enabled the live
    /// path is still guarded (the treasury endpoint is wired in deliberately by
    /// an operator), so a flipped flag alone never silently spends funds.
    async fn maybe_auto_fund(compose: &std::path::Path) {
        let Some(addr) = Self::node_address(compose) else {
            warn!("Sentinel node address unavailable — skipping funding check");
            return;
        };
        if !Self::auto_fund_enabled() {
            info!(
                node = %addr,
                min_udvpn = Self::MIN_FUNDING_UDVPN,
                "Sentinel node would be auto-funded from the Fry treasury (auto-funding gated — no live transfer this build)"
            );
            return;
        }
        // Auto-funding explicitly enabled. The live treasury transfer requires a
        // funding endpoint that is intentionally not wired in this build, so we
        // do NOT move funds even when the flag is set — surface the intent and
        // the target so an operator can complete the wiring deliberately.
        warn!(
            node = %addr,
            min_udvpn = Self::MIN_FUNDING_UDVPN,
            "Sentinel auto-funding is enabled but the treasury funding path is not wired in this build — NOT sending. Fund this node manually or complete the treasury endpoint."
        );
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
            .output_bounded(crate::supervisor::platform::LONG_TIMEOUT)?;

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

        // install() only runs when installed_version() is None, so an existing
        // deploy dir would otherwise pin a stale compose file forever. Refresh
        // it from the shipped content, then make sure the volume really holds a
        // config + `main` key (both are required for `dvpnx start`).
        tokio::fs::write(&compose, include_str!("sentinel_deploy/docker-compose.yml")).await?;
        if Self::node_address(&compose).is_none() {
            Self::init_node_config(&compose).await?;
        }

        info!("Starting Sentinel dVPN containers");
        let output = crate::supervisor::platform::command("docker")
            .args(["compose", "-f", &compose.to_string_lossy(), "up", "-d"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output_bounded(crate::supervisor::platform::LONG_TIMEOUT)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(stderr = %stderr, "Failed to start Sentinel");
            anyhow::bail!("Failed to start Sentinel dVPN: {}", tail_lines(&stderr, 15));
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        // F16: the node now has a Fry-managed wallet — check it and (gated)
        // top it up from the treasury so it can register on chain.
        Self::maybe_auto_fund(&compose).await;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        let compose = compose_file();
        if compose.exists() {
            crate::supervisor::platform::command("docker")
                .args(["compose", "-f", &compose.to_string_lossy(), "stop"])
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)?;
            info!("Stopped Sentinel dVPN containers");
        }
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        if !docker_available() {
            // State-aware guidance ("not installed" vs "not running" vs
            // VM/BIOS virtualization) instead of a generic string.
            return HealthStatus::Unhealthy(super::docker_manager::status_user_message(
                super::docker_manager::docker_status(),
            ));
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
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
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
                // `docker compose ps --format json` emits NDJSON (one object per
                // line), not a JSON array — parse both so a running node is not
                // reported unhealthy just because the shape changed.
                let parsed: Option<Vec<serde_json::Value>> =
                    serde_json::from_str::<Vec<serde_json::Value>>(&stdout)
                        .ok()
                        .or_else(|| {
                            let rows: Vec<serde_json::Value> = stdout
                                .lines()
                                .map(str::trim)
                                .filter(|l| l.starts_with('{'))
                                .filter_map(|l| serde_json::from_str(l).ok())
                                .collect();
                            if rows.is_empty() { None } else { Some(rows) }
                        });
                if let Some(containers) = parsed {
                    let states: Vec<String> = containers
                        .iter()
                        .filter_map(|c| c.get("State").and_then(|s| s.as_str()))
                        .map(|s| s.to_string())
                        .collect();

                    match classify_compose_states(&states) {
                        ComposeVerdict::Running => return HealthStatus::Healthy,
                        ComposeVerdict::NoContainers => {
                            // Compose project exists but its containers were
                            // removed — Unhealthy so the supervisor restart
                            // path re-runs `compose up -d` and recreates them.
                            return HealthStatus::Unhealthy(
                                "No Sentinel containers exist — restarting to recreate them"
                                    .to_string(),
                            );
                        }
                        ComposeVerdict::Stuck(state) => {
                            return HealthStatus::Unhealthy(format!(
                                "Sentinel container is {state} and cannot progress — restarting"
                            ));
                        }
                        ComposeVerdict::Starting => return HealthStatus::Starting,
                        ComposeVerdict::Exited => { /* fall through to log fetch below */ }
                    }

                    {
                        // Try to get logs for more detail
                        if let Ok(log_output) = crate::supervisor::platform::command("docker")
                            .args([
                                "compose",
                                "-f",
                                &compose.to_string_lossy(),
                                "logs",
                                "sentinel-dvpnx",
                            ])
                            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
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

#[cfg(test)]
mod tests {
    use super::*;

    // v0.4.8 stuck-STARTING repro: a container that exists but is stopped
    // ("exited"/"dead"), paused, or wholly removed left the card on
    // "Starting" forever — the supervisor never restarted it because
    // Starting is not Unhealthy.
    #[test]
    fn dead_container_is_exited_not_starting() {
        assert_eq!(
            classify_compose_states(&["dead".to_string()]),
            ComposeVerdict::Exited
        );
    }

    #[test]
    fn no_containers_is_its_own_verdict_not_starting() {
        assert_eq!(classify_compose_states(&[]), ComposeVerdict::NoContainers);
    }

    #[test]
    fn paused_container_is_stuck_not_starting() {
        assert_eq!(
            classify_compose_states(&["paused".to_string()]),
            ComposeVerdict::Stuck("paused".to_string())
        );
    }

    #[test]
    fn exited_container_is_exited() {
        assert_eq!(
            classify_compose_states(&["exited (1)".to_string()]),
            ComposeVerdict::Exited
        );
    }

    #[test]
    fn running_wins_over_everything() {
        assert_eq!(
            classify_compose_states(&["exited".to_string(), "running".to_string()]),
            ComposeVerdict::Running
        );
    }

    #[test]
    fn created_and_restarting_are_genuinely_starting() {
        assert_eq!(
            classify_compose_states(&["created".to_string()]),
            ComposeVerdict::Starting
        );
        assert_eq!(
            classify_compose_states(&["restarting".to_string()]),
            ComposeVerdict::Starting
        );
    }
}
