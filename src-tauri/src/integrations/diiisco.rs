use crate::supervisor::platform::BoundedOutput;
use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};
use crate::api::client::ApiClient;
use crate::api::types::CredentialInfo;
use crate::config::store::ConfigStore;

const HEALTH_URL: &str = "http://localhost:8181/health";

/// B6: what the user is told when hardwareapi has no Algorand wallet for this
/// device yet. A missing wallet is a provisioning state, not a fault — the old
/// "Device has no Algorand wallet — re-register or contact support" read as a
/// crash and gave no first step.
pub const WALLET_NOT_PROVISIONED: &str =
    "Device wallet not provisioned yet — open Settings and re-register this device, or contact support.";

/// Sticky, process-wide record of the wallet condition.
///
/// `check_requirements()` is synchronous — the PoC reporter calls it on every
/// tick — so it can never do the credentials lookup itself. This flag is how
/// the async discovery in `install()`/`start()` reaches it. Routing the
/// condition through `unavailable_reason` instead of a start error is the
/// whole point: the card goes inert with a sentence the user can act on, the
/// toggle refuses up front, and the health loop stops restarting something
/// that provably cannot start yet.
static WALLET_MISSING: AtomicBool = AtomicBool::new(false);

/// Classify a credentials lookup into the wallet address or the user-facing
/// reason the integration cannot run yet. A blank address counts as missing:
/// hardwareapi returns `""` for a device whose wallet row exists but has not
/// been funded with an address.
pub fn classify_wallet(algo_address: Option<&str>) -> Result<String, &'static str> {
    match algo_address {
        Some(addr) if !addr.trim().is_empty() => Ok(addr.trim().to_string()),
        _ => Err(WALLET_NOT_PROVISIONED),
    }
}

/// Record the observed wallet state. Logs only on a transition, so a device
/// waiting on provisioning does not write a line every health tick.
fn record_wallet_state(provisioned: bool) {
    let was_missing = WALLET_MISSING.swap(!provisioned, Ordering::Relaxed);
    match (provisioned, was_missing) {
        (false, false) => info!("Diiisco unavailable: {}", WALLET_NOT_PROVISIONED),
        (true, true) => info!("Diiisco: device wallet is now provisioned"),
        _ => {}
    }
}

/// True while the device is known to have no Algorand wallet.
pub fn wallet_missing() -> bool {
    WALLET_MISSING.load(Ordering::Relaxed)
}

/// The `check_requirements` answer for a given wallet state. Split out so it
/// can be tested without constructing a `DiiiscoIntegration`, which needs a
/// live `ApiClient` and `ConfigStore`.
pub fn requirements_for(wallet_is_missing: bool) -> Result<(), String> {
    if wallet_is_missing {
        return Err(WALLET_NOT_PROVISIONED.to_string());
    }
    Ok(())
}
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

/// Fetch credentials with exponential-backoff retry on network errors.
/// Retries up to 3 times (2s, 4s, 8s delay) for connection-level failures
/// only — HTTP error responses are NOT retried.
async fn fetch_credentials_with_retry(
    api_client: &ApiClient,
    miner_key: &str,
) -> std::result::Result<CredentialInfo, crate::api::client::ApiError> {
    let mut result = crate::api::credentials::lookup(api_client, miner_key).await;
    for attempt in 1..=3u32 {
        match &result {
            Err(crate::api::client::ApiError::Request(_)) => {
                let delay = 2u64.pow(attempt);
                warn!(attempt, delay_secs = delay, "Credential fetch failed (network error), retrying");
                tokio::time::sleep(Duration::from_secs(delay)).await;
                result = crate::api::credentials::lookup(api_client, miner_key).await;
            }
            _ => break,
        }
    }
    result
}

fn docker_available() -> bool {
    super::docker_manager::docker_cli_probe_bounded() == Some(true)
}

fn compose_file() -> PathBuf {
    deploy_dir().join("docker-compose.yml")
}

/// The diiisco-node image is built locally by install(); it is never
/// published to a registry, so `compose up` must not be allowed to fall back
/// to pulling it.
fn image_built() -> bool {
    crate::supervisor::platform::command("docker")
        .args(["images", "-q", "diiisco-node:latest"])
        .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false)
}

/// Last `n` lines of subprocess output — full walls of Docker logs go to the
/// log file, not the UI error banner.
fn tail_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
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
        // Ensure Docker is available, auto-installing if needed
        super::docker_manager::ensure_docker().await?;

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
        let creds = fetch_credentials_with_retry(&self.api_client, miner_key).await
            .map_err(|e| anyhow::anyhow!("Failed to fetch credentials: {}", e))?;
        let algo_address = match classify_wallet(creds.algo_address.as_deref()) {
            Ok(addr) => {
                record_wallet_state(true);
                addr
            }
            Err(reason) => {
                record_wallet_state(false);
                anyhow::bail!("{}", reason);
            }
        };
        let algo_mnemonic = creds.algo_mnemonic.ok_or_else(|| {
            anyhow::anyhow!("Device Algorand mnemonic unavailable — contact support")
        })?;

        // Docker compose build with credentials as env vars (NOT command-line args)
        let bearer = diiisco_bearer_token();
        if bearer.is_empty() {
            anyhow::bail!("DIIISCO_BEARER_TOKEN not configured — set the environment variable before enabling Diiisco");
        }
        info!("Building Diiisco Docker image");
        let output = crate::supervisor::platform::command("docker")
            .args(["compose", "build"])
            .env("ALGO_ADDRESS", &algo_address)
            .env("ALGO_MNEMONIC", &algo_mnemonic)
            .env("DIIISCO_API_KEY", bearer)
            .current_dir(&deploy_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output_bounded(crate::supervisor::platform::LONG_TIMEOUT)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(stderr = %stderr, "docker compose build failed (full output)");
            anyhow::bail!("docker compose build failed: {}", tail_lines(&stderr, 15));
        }
        info!("Diiisco install complete — image built with per-device wallet");
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        super::docker_manager::ensure_docker().await?;
        let compose = compose_file();
        if !compose.exists() {
            anyhow::bail!(
                "Diiisco is not installed yet (deploy directory missing at {}) — toggle it off and on to reinstall",
                deploy_dir().display()
            );
        }

        // Refresh the compose from the embedded copy on every start. Only
        // install() used to write it, and installed_version() reports installed
        // whenever the file exists — so a machine that installed an older build
        // would keep its stale compose forever and never pick up fixes like
        // dropping the conflicting 11434 publish.
        tokio::fs::write(
            &compose,
            include_str!("diiisco_deploy/docker-compose.yml"),
        )
        .await?;

        // The compose file interpolates ${ALGO_ADDRESS}/${ALGO_MNEMONIC}/
        // ${DIIISCO_API_KEY} on EVERY invocation, `up` included — without
        // them compose warns "variable is not set" and passes blank build
        // args. Fetch the same credentials install() uses.
        let cfg = self.config.get();
        let miner_key = cfg.miner_key.as_deref().ok_or_else(|| {
            anyhow::anyhow!("Miner key not set — complete device registration before starting Diiisco")
        })?;
        let creds = fetch_credentials_with_retry(&self.api_client, miner_key).await
            .map_err(|e| anyhow::anyhow!("Failed to fetch credentials: {}", e))?;
        let algo_address = match classify_wallet(creds.algo_address.as_deref()) {
            Ok(addr) => {
                record_wallet_state(true);
                addr
            }
            Err(reason) => {
                record_wallet_state(false);
                anyhow::bail!("{}", reason);
            }
        };
        let algo_mnemonic = creds.algo_mnemonic.ok_or_else(|| {
            anyhow::anyhow!("Device Algorand mnemonic unavailable — contact support")
        })?;
        let bearer = diiisco_bearer_token();
        if bearer.is_empty() {
            anyhow::bail!("DIIISCO_BEARER_TOKEN not configured — set the environment variable before enabling Diiisco");
        }

        let deploy = deploy_dir();

        // diiisco-node is built locally, never pulled. If the image is
        // missing (failed/interrupted install), `up` would try to pull it
        // from a registry — "pull access denied". Build first.
        if !image_built() {
            info!("diiisco-node image missing — building before start");
            let output = crate::supervisor::platform::command("docker")
                .args(["compose", "build"])
                .env("ALGO_ADDRESS", &algo_address)
                .env("ALGO_MNEMONIC", &algo_mnemonic)
                .env("DIIISCO_API_KEY", &bearer)
                .current_dir(&deploy)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output_bounded(crate::supervisor::platform::LONG_TIMEOUT)?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!(stderr = %stderr, "Diiisco pre-start image build failed (full output)");
                anyhow::bail!("Diiisco image build failed: {}", tail_lines(&stderr, 15));
            }
        }

        // Best-effort cleanup of stale containers/networks from prior failed
        // runs ("network diiisco_default already exists"). Volumes survive.
        // Failures don't block startup, but they must be visible in logs.
        info!("Cleaning up stale Diiisco containers/networks");
        match crate::supervisor::platform::command("docker")
            .args(["compose", "down", "--remove-orphans"])
            .env("ALGO_ADDRESS", &algo_address)
            .env("ALGO_MNEMONIC", &algo_mnemonic)
            .env("DIIISCO_API_KEY", &bearer)
            .current_dir(&deploy)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
        {
            Ok(o) if !o.status.success() => {
                warn!(
                    stderr = %String::from_utf8_lossy(&o.stderr),
                    "Diiisco pre-start cleanup reported errors (continuing)"
                );
            }
            Err(e) => {
                warn!(error = %e, "Diiisco pre-start cleanup failed to run (continuing)");
            }
            _ => {}
        }

        info!("Starting Diiisco containers");
        let output = crate::supervisor::platform::command("docker")
            .args(["compose", "up", "-d"])
            .env("ALGO_ADDRESS", &algo_address)
            .env("ALGO_MNEMONIC", &algo_mnemonic)
            .env("DIIISCO_API_KEY", &bearer)
            .current_dir(&deploy)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output_bounded(crate::supervisor::platform::LONG_TIMEOUT)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(stderr = %stderr, "Failed to start Diiisco (full output)");
            anyhow::bail!("Failed to start Diiisco: {}", tail_lines(&stderr, 15));
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        let compose = compose_file();
        if compose.exists() {
            crate::supervisor::platform::command("docker")
                .args(["compose", "-f", &compose.to_string_lossy(), "stop"])
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)?;
            info!("Stopped Diiisco containers");
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
        if !compose_file().exists() {
            return None;
        }
        // Compose file present but image never built = failed/interrupted
        // install — report not-installed so the toggle path re-runs install.
        // Only checkable when the Docker engine is reachable.
        if docker_available() && !image_built() {
            return None;
        }
        Some("installed".into())
    }

    fn collect_poc_data(&self) -> PocGateData {
        // Synchronous check — can't do async health here, check compose file existence
        let compose_exists = compose_file().exists();
        PocGateData {
            poa: compose_exists && docker_available(),
            ..Default::default()
        }
    }

    fn requires_docker(&self) -> bool {
        true
    }

    /// B6: a device whose wallet has not been provisioned yet cannot run
    /// Diiisco, and saying so here is what makes the card inert (with the
    /// reason) instead of leaving the user staring at a red start error the
    /// health loop keeps re-triggering. Self-clearing: the next successful
    /// credentials lookup resets the flag.
    fn check_requirements(&self) -> Result<(), String> {
        requirements_for(wallet_missing())
    }
}

#[cfg(test)]
mod tests {
    const COMPOSE: &str = include_str!("diiisco_deploy/docker-compose.yml");

    #[test]
    fn ollama_publishes_no_host_port() {
        // Publishing 11434 collided with any Ollama the user already runs and
        // killed `docker compose up` with a bind error. Nothing outside the
        // compose network needs it.
        assert!(
            !COMPOSE.contains("11434:11434"),
            "the ollama host port publish must stay removed"
        );
    }

    #[test]
    fn ollama_is_still_reachable_inside_the_compose_network() {
        // Removing the publish is only safe while consumers use service DNS.
        assert!(COMPOSE.contains("http://ollama:11434"));
    }

    #[test]
    fn diiisco_still_publishes_its_api_port() {
        // health_check() hits http://localhost:8181/health from the host, so
        // this publish is required.
        assert!(COMPOSE.contains("8181:8181"));
    }

    #[test]
    fn ollama_service_and_healthcheck_survived_the_edit() {
        assert!(COMPOSE.contains("container_name: diiisco-ollama"));
        assert!(COMPOSE.contains(r#"["CMD", "ollama", "list"]"#));
    }
}

/// B6: an unprovisioned device wallet is a state to explain, not a crash.
#[cfg(test)]
mod wallet_condition_tests {
    use super::*;

    #[test]
    fn a_real_address_is_accepted_and_trimmed() {
        let addr = "TESTWALLETADDRESSTESTWALLETADDRESSTESTWALLETADDRESSTESTWA";
        assert_eq!(classify_wallet(Some(addr)).unwrap(), addr);
        assert_eq!(classify_wallet(Some("  ABC  ")).unwrap(), "ABC");
    }

    #[test]
    fn a_missing_address_maps_to_the_provisioning_message() {
        assert_eq!(classify_wallet(None), Err(WALLET_NOT_PROVISIONED));
    }

    #[test]
    fn a_blank_address_counts_as_not_provisioned() {
        // hardwareapi returns "" for a device row created before its wallet.
        assert_eq!(classify_wallet(Some("")), Err(WALLET_NOT_PROVISIONED));
        assert_eq!(classify_wallet(Some("   ")), Err(WALLET_NOT_PROVISIONED));
    }

    #[test]
    fn the_message_tells_the_user_where_to_go_first() {
        // The old text ("re-register or contact support") named no screen.
        assert!(WALLET_NOT_PROVISIONED.contains("Settings"));
        assert!(WALLET_NOT_PROVISIONED.contains("re-register"));
        // It must not read like a failure — that is what drove the support load.
        let lower = WALLET_NOT_PROVISIONED.to_lowercase();
        assert!(!lower.contains("failed"));
        assert!(!lower.contains("error"));
    }

    #[test]
    fn the_requirement_gate_reports_exactly_the_same_reason() {
        // Whatever the user sees as unavailable_reason has to be this string,
        // or the card and the toggle rejection disagree.
        assert_eq!(requirements_for(true), Err(WALLET_NOT_PROVISIONED.to_string()));
        assert_eq!(requirements_for(false), Ok(()));
    }

    #[test]
    fn the_observed_state_drives_the_gate_and_clears_itself() {
        // One test owns the process-wide flag so parallel siblings cannot
        // observe it mid-transition.
        record_wallet_state(false);
        assert!(wallet_missing());
        assert_eq!(
            requirements_for(wallet_missing()),
            Err(WALLET_NOT_PROVISIONED.to_string())
        );

        record_wallet_state(true);
        assert!(!wallet_missing(), "a provisioned wallet must clear the flag");
        assert_eq!(requirements_for(wallet_missing()), Ok(()));
    }
}
