use super::{HealthStatus, Integration, PocGateData};
use crate::config::store::ConfigStore;
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{info, warn};

const FRYNODE_VERSION: &str = "0.1.0";

pub struct FryVpnIntegration {
    pub config: Arc<ConfigStore>,
    pub supervisor: Arc<Mutex<crate::supervisor::Supervisor>>,
}

impl FryVpnIntegration {
    fn binary_name() -> &'static str {
        if cfg!(target_os = "windows") {
            "frynode.exe"
        } else {
            "frynode"
        }
    }

    /// Where the bundled resource actually lands: alongside the running
    /// executable, under `resources/`. True for the installed layout
    /// (`…\Fry Edge Miner\resources\frynode.exe`) and for `cargo run`
    /// (`target/<profile>/resources/frynode.exe`).
    fn bundled_candidate() -> Option<PathBuf> {
        let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
        Some(dir.join("resources").join(Self::binary_name()))
    }

    /// Pure precedence, so the ordering is testable without touching the disk.
    /// `resource` is passed only when it exists.
    fn resolve_binary(env_override: Option<String>, resource: Option<PathBuf>) -> String {
        if let Some(o) = env_override.filter(|s| !s.trim().is_empty()) {
            return o;
        }
        match resource {
            Some(p) => p.to_string_lossy().to_string(),
            // Last resort: a bare name, so a frynode installed on PATH still runs.
            None => Self::binary_name().to_string(),
        }
    }

    /// frynode refuses to start without a region ("failed to load config:
    /// REGION is required") and FEM has no region concept of its own, so this
    /// supplies a default that `FRYNODE_REGION` can override.
    fn region() -> String {
        std::env::var("FRYNODE_REGION")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "us".to_string())
    }

    /// The other config frynode insists on: "CAPACITY_MBPS is required
    /// (must be > 0)". Probing the binary directly showed region plus a
    /// non-zero capacity is the complete required set — price-per-gb is
    /// optional. Overridable via `FRYNODE_CAPACITY_MBPS`.
    fn capacity_mbps() -> u32 {
        std::env::var("FRYNODE_CAPACITY_MBPS")
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(100)
    }

    /// Resolve the frynode binary: `FRYNODE_BIN` → the bundled resource next to
    /// the executable → the bare name on PATH.
    ///
    /// This used to return the bare name unconditionally, so `Command::new`
    /// searched `%PATH%`, found nothing, and every start failed with
    /// "program not found" — even though the binary ships with the app.
    fn binary_path() -> Result<String> {
        let resource = Self::bundled_candidate().filter(|p| p.exists());
        Ok(Self::resolve_binary(
            std::env::var("FRYNODE_BIN").ok(),
            resource,
        ))
    }
}

/// One HTTP probe of the local frynode `/health` endpoint. Extracted so the
/// health check can retry across the warm-up window (F5).
async fn probe_health_once() -> HealthStatus {
    let client = reqwest::Client::new();
    let health_url = "http://127.0.0.1:8088/health";

    match tokio::time::timeout(Duration::from_secs(5), client.get(health_url).send()).await {
        Ok(Ok(resp)) if resp.status().is_success() => {
            match resp.json::<serde_json::Value>().await {
                Ok(body) => {
                    let is_healthy = body
                        .get("status")
                        .and_then(|v| v.as_str())
                        .map(|s| s == "healthy")
                        .unwrap_or(false);
                    let is_registered = body
                        .get("registered")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    if is_healthy && is_registered {
                        HealthStatus::Healthy
                    } else if !is_healthy {
                        HealthStatus::Unhealthy("dVPN health check: status != healthy".to_string())
                    } else {
                        HealthStatus::Unhealthy("dVPN not registered on-chain".to_string())
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Failed to parse health response");
                    HealthStatus::Unhealthy("Invalid health response format".to_string())
                }
            }
        }
        Ok(Ok(_)) => HealthStatus::Unhealthy("Health check returned non-200".to_string()),
        Ok(Err(e)) => {
            warn!(error = %e, "Health check request failed");
            HealthStatus::Unhealthy(format!("Health check error: {}", e))
        }
        Err(_) => {
            warn!("Health check timeout");
            HealthStatus::Unhealthy("Health check timeout".to_string())
        }
    }
}

#[async_trait]
impl Integration for FryVpnIntegration {
    fn id(&self) -> &str {
        "fryvpn"
    }

    fn display_name(&self) -> &str {
        "Fry dVPN"
    }

    async fn install(&self) -> Result<()> {
        // Nothing to download — frynode ships with the app. Verify it is really
        // there rather than reporting success and failing later at start().
        let binary = Self::binary_path()?;
        let path = std::path::Path::new(&binary);
        if path.is_absolute() && !path.exists() {
            anyhow::bail!("frynode not found at {}", path.display());
        }
        info!(binary = %binary, "Fry dVPN binary found");
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        let binary = Self::binary_path()?;

        // Build CLI flags for frynode
        let args = vec![
            "-registry-app-id".to_string(),
            "3636586918".to_string(),
            "-fvpn-asa-id".to_string(),
            "2485198745".to_string(),
            "-algod-server".to_string(),
            "https://mainnet-api.algonode.cloud".to_string(),
            "-algod-port".to_string(),
            "443".to_string(),
            "-algod-token".to_string(),
            "".to_string(), // algonode is tokenless
            "-api-port".to_string(),
            "8088".to_string(),
            "-wg-port".to_string(),
            "51820".to_string(),
            "-region".to_string(),
            Self::region(),
            "-capacity-mbps".to_string(),
            Self::capacity_mbps().to_string(),
        ];

        {
            let mut sup = self.supervisor.lock().unwrap();
            let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            sup.start_integration("fryvpn", &binary, &arg_refs)
                .map_err(|e| anyhow::anyhow!("Failed to spawn frynode: {}", e))?;
        }

        info!("Fry dVPN started with CLI flags");
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        {
            let mut sup = self.supervisor.lock().unwrap();
            sup.stop_integration("fryvpn")
                .map_err(|e| anyhow::anyhow!("Failed to stop frynode: {}", e))?;
        }
        info!("Fry dVPN stopped");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        // Check process alive first
        let process_alive = {
            let mut sup = self.supervisor.lock().unwrap();
            matches!(sup.get_status("fryvpn"), HealthStatus::Healthy)
        };

        if !process_alive {
            return HealthStatus::Stopped;
        }

        // F5: the frynode HTTP endpoint and its on-chain registration both settle
        // a beat after the process starts, so a single probe races the warm-up
        // and flips the card to Unhealthy — arming a needless restart. Retry a
        // few times and only surface the last failure if none succeed.
        let mut last = HealthStatus::Unhealthy("dVPN health check pending".to_string());
        for attempt in 0..3u32 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            match probe_health_once().await {
                HealthStatus::Healthy => return HealthStatus::Healthy,
                other => last = other,
            }
        }
        last
    }

    async fn check_update(&self) -> Result<Option<String>> {
        // No built-in update mechanism
        Ok(None)
    }

    fn installed_version(&self) -> Option<String> {
        if Self::binary_path().is_ok() {
            Some(FRYNODE_VERSION.to_string())
        } else {
            None
        }
    }

    fn collect_poc_data(&self) -> PocGateData {
        let status = {
            let mut sup = self.supervisor.lock().unwrap();
            sup.get_status("fryvpn")
        };
        PocGateData {
            poa: matches!(status, HealthStatus::Healthy),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fryvpn_id() {
        let integration = FryVpnIntegration {
            config: Arc::new(crate::config::store::ConfigStore::new(std::path::PathBuf::from("/tmp"))),
            supervisor: Arc::new(Mutex::new(crate::supervisor::Supervisor::new(
                std::path::PathBuf::from("/tmp"),
            ))),
        };
        assert_eq!(integration.id(), "fryvpn");
    }

    #[test]
    fn test_fryvpn_display_name() {
        let integration = FryVpnIntegration {
            config: Arc::new(crate::config::store::ConfigStore::new(std::path::PathBuf::from("/tmp"))),
            supervisor: Arc::new(Mutex::new(crate::supervisor::Supervisor::new(
                std::path::PathBuf::from("/tmp"),
            ))),
        };
        assert_eq!(integration.display_name(), "Fry dVPN");
    }

    #[test]
    fn test_fryvpn_binary_not_found() {
        // Suppress FRYNODE_BIN and ensure "notareal_frynode" isn't on PATH
        std::env::remove_var("FRYNODE_BIN");
        // This test will fail if frynode is somehow on PATH, which is expected
        // since we're testing the error path
        // In CI, this should pass
        let _ = FryVpnIntegration::binary_path();
        // We can't easily assert the error without a more complex setup,
        // so we just verify the function runs
    }

    #[test]
    fn env_override_wins_over_the_bundled_resource() {
        let resolved = FryVpnIntegration::resolve_binary(
            Some("D:/custom/frynode.exe".to_string()),
            Some(PathBuf::from("C:/app/resources/frynode.exe")),
        );
        assert_eq!(resolved, "D:/custom/frynode.exe");
    }

    #[test]
    fn resolves_to_the_bundled_resource_when_no_override() {
        // The shipped bug: this returned the bare name, so Command::new searched
        // %PATH%, found nothing, and start() failed with "program not found"
        // even though the binary sits next to the executable.
        let resolved = FryVpnIntegration::resolve_binary(
            None,
            Some(PathBuf::from("C:/app/resources/frynode.exe")),
        );
        assert!(resolved.ends_with("frynode.exe"), "{resolved}");
        assert!(resolved.contains("resources"), "must be the full resource path: {resolved}");
    }

    #[test]
    fn blank_override_is_ignored() {
        let resolved = FryVpnIntegration::resolve_binary(
            Some("   ".to_string()),
            Some(PathBuf::from("C:/app/resources/frynode.exe")),
        );
        assert!(resolved.contains("resources"), "{resolved}");
    }

    #[test]
    fn region_defaults_when_unset_and_is_never_empty() {
        // frynode exits with "failed to load config: REGION is required" if this
        // is missing, which is what kept the binary from staying up.
        std::env::remove_var("FRYNODE_REGION");
        assert_eq!(FryVpnIntegration::region(), "us");
    }

    #[test]
    fn region_honours_the_env_override() {
        std::env::set_var("FRYNODE_REGION", "eu-west");
        assert_eq!(FryVpnIntegration::region(), "eu-west");
        std::env::set_var("FRYNODE_REGION", "   ");
        assert_eq!(FryVpnIntegration::region(), "us", "blank override must fall back");
        std::env::remove_var("FRYNODE_REGION");
    }

    #[test]
    fn capacity_is_always_positive() {
        // frynode rejects 0 outright: "CAPACITY_MBPS is required (must be > 0)".
        std::env::remove_var("FRYNODE_CAPACITY_MBPS");
        assert!(FryVpnIntegration::capacity_mbps() > 0);

        std::env::set_var("FRYNODE_CAPACITY_MBPS", "250");
        assert_eq!(FryVpnIntegration::capacity_mbps(), 250);

        for bad in ["0", "-5", "abc", ""] {
            std::env::set_var("FRYNODE_CAPACITY_MBPS", bad);
            assert!(
                FryVpnIntegration::capacity_mbps() > 0,
                "override {bad:?} must not produce a zero capacity"
            );
        }
        std::env::remove_var("FRYNODE_CAPACITY_MBPS");
    }

    #[test]
    fn falls_back_to_the_bare_name_when_no_resource_is_present() {
        // Keeps a PATH-installed frynode working.
        let resolved = FryVpnIntegration::resolve_binary(None, None);
        assert_eq!(resolved, FryVpnIntegration::binary_name());
    }
}
