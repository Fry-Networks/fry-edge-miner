use super::download::partners_base_dir;
use super::{HealthStatus, Integration, PocGateData};
use crate::api::client::ApiClient;
use crate::config::store::ConfigStore;
use crate::supervisor::Supervisor;
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::info;

/// Log level passed to sdk_client.exe — confirmed from live NSSM earner's AppParameters.
const SDK_LOG_LEVEL: &str = "info";

/// QUIC connection success marker (verbatim from live earner stderr, Go zerolog format).
const QUIC_CONNECTED_MARKER: &str = "Connected to QUIC server";

pub struct MysteriumIntegration {
    pub api_client: Arc<ApiClient>,
    pub config: Arc<ConfigStore>,
    pub supervisor: Arc<Mutex<Supervisor>>,
    pub log_dir: PathBuf,
}

impl MysteriumIntegration {
    fn partner_dir() -> PathBuf {
        partners_base_dir().join("mysterium")
    }

    fn binary_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        return Self::partner_dir().join("sdk_client.exe");
        #[cfg(not(target_os = "windows"))]
        return Self::partner_dir().join("sdk_client");
    }

    /// Strip ANSI escape sequences from a log line (zerolog colored output).
    fn strip_ansi(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut in_escape = false;
        for c in s.chars() {
            if c == '\x1b' {
                in_escape = true;
            } else if in_escape {
                if c == 'm' {
                    in_escape = false;
                }
            } else {
                result.push(c);
            }
        }
        result
    }
}

#[async_trait]
impl Integration for MysteriumIntegration {
    fn id(&self) -> &str {
        "mysterium"
    }

    fn display_name(&self) -> &str {
        "MystNodes"
    }

    async fn install(&self) -> Result<()> {
        let binary = Self::binary_path();
        if binary.exists() {
            info!(path = ?binary, "sdk_client binary already present");
            return Ok(());
        }
        // TODO: backend partner-binary distribution for sdk_client pending
        anyhow::bail!(
            "sdk_client binary not found at {} — backend partner-binary distribution pending",
            binary.display()
        );
    }

    async fn start(&self) -> Result<()> {
        let binary = Self::binary_path();
        if !binary.exists() {
            anyhow::bail!("sdk_client binary not found at {}", binary.display());
        }

        // Credential fetch (Diiisco pattern)
        let cfg = self.config.get();
        let miner_key = cfg.miner_key.as_deref().ok_or_else(|| {
            anyhow::anyhow!("Miner key not set — complete device registration before starting MystNodes")
        })?;

        let creds = crate::api::credentials::lookup(&self.api_client, miner_key)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch credentials: {}", e))?;

        // Extract mystnodes_user_token — fail-closed if missing (no user-claimable fallback)
        let token = creds
            .mystnodes_user_token
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "mystnodes_user_token not found or empty in device credentials — \
                     contact support to provision a Mysterium token for this device"
                )
            })?;

        let binary_str = binary.to_string_lossy().to_string();
        let token_arg = format!("--user.token={}", token);
        let log_level_arg = format!("--log.level={}", SDK_LOG_LEVEL);
        let args = [token_arg.as_str(), log_level_arg.as_str()];

        // Lock supervisor in explicit scope — guard drops at }
        {
            let mut sup = self.supervisor.lock().unwrap();
            sup.start_integration("mysterium", &binary_str, &args)
                .map_err(|e| anyhow::anyhow!("Failed to spawn sdk_client: {}", e))?;
        }

        info!("Mysterium SDK client started (token redacted)");
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        {
            let mut sup = self.supervisor.lock().unwrap();
            sup.stop_integration("mysterium")
                .map_err(|e| anyhow::anyhow!("Failed to stop sdk_client: {}", e))?;
        }
        info!("Mysterium SDK client stopped");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        // Check process alive — guard drops at }
        let process_alive = {
            let mut sup = self.supervisor.lock().unwrap();
            matches!(sup.get_status("mysterium"), HealthStatus::Healthy)
        };

        if !process_alive {
            return HealthStatus::Stopped;
        }

        // Read BOTH log files (sdk_client writes to stderr; stdout typically empty)
        let stdout_path = self.log_dir.join("mysterium").join("mysterium_stdout.log");
        let stderr_path = self.log_dir.join("mysterium").join("mysterium_stderr.log");

        let stdout_content = tokio::fs::read_to_string(&stdout_path).await.unwrap_or_default();
        let stderr_content = tokio::fs::read_to_string(&stderr_path).await.unwrap_or_default();

        // Combine recent tail (last 50 lines of each)
        let recent_lines: Vec<String> = stdout_content
            .lines()
            .chain(stderr_content.lines())
            .rev()
            .take(50)
            .map(Self::strip_ansi)
            .collect();

        // Check for error markers — anchored, space-bounded (zerolog: INF/WRN/ERR/FTL)
        let has_errors = recent_lines
            .iter()
            .any(|l| l.contains(" ERR ") || l.contains(" FTL "));

        if has_errors {
            return HealthStatus::Unhealthy("Error detected in SDK client logs".to_string());
        }

        // Check for QUIC connection success
        let quic_connected = recent_lines
            .iter()
            .any(|l| l.contains(QUIC_CONNECTED_MARKER));

        if quic_connected {
            HealthStatus::Healthy
        } else {
            // Process up but QUIC not yet connected
            HealthStatus::Starting
        }
    }

    async fn check_update(&self) -> Result<Option<String>> {
        // TODO: partner binary version check via /versions/mysterium
        Ok(None)
    }

    fn installed_version(&self) -> Option<String> {
        if Self::binary_path().exists() {
            Some("installed".into())
        } else {
            None
        }
    }

    fn collect_poc_data(&self) -> PocGateData {
        // Sync — supervisor status only (process-alive, sibling convention)
        let status = {
            let mut sup = self.supervisor.lock().unwrap();
            sup.get_status("mysterium")
        };
        PocGateData {
            poa: matches!(status, HealthStatus::Healthy),
            ..Default::default()
        }
    }
}
