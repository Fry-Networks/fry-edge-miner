use super::download::{download_file_with_options, partners_base_dir};
use super::{HealthStatus, Integration, PocGateData};
use crate::api::client::ApiClient;
use crate::config::store::ConfigStore;
use crate::supervisor::Supervisor;
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{info, warn};

const GITHUB_API_URL: &str = "https://api.github.com/repos/Fry-Foundation/fem-partner-binaries/releases/latest";
const USER_AGENT: &str = concat!("FryEdgeMiner/", env!("CARGO_PKG_VERSION"));

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

    fn github_token() -> Option<String> {
        std::env::var("GITHUB_TOKEN").ok().filter(|s| !s.is_empty())
    }

    fn build_client() -> reqwest::Client {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(token) = Self::github_token() {
            headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token))
                    .expect("invalid GITHUB_TOKEN header value"),
            );
        }

        reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .user_agent(USER_AGENT)
            .default_headers(headers)
            .build()
            .expect("failed to build GitHub HTTP client")
    }

    async fn fetch_latest_release() -> Result<(String, String)> {
        let client = Self::build_client();
        let max_attempts = 3u32;
        let base_delay = Duration::from_secs(2);
        let mut last_error = None;

        for attempt in 1..=max_attempts {
            info!(url = GITHUB_API_URL, attempt = attempt, "Fetching latest MystNodes release");

            match client.get(GITHUB_API_URL).send().await {
                Ok(response) => {
                    let status = response.status();

                    if status.is_success() {
                        let json: serde_json::Value = response.json().await?;
                        let tag_name = json["tag_name"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("No tag_name in release"))?
                            .to_string();

                        let assets = json["assets"]
                            .as_array()
                            .ok_or_else(|| anyhow::anyhow!("No assets in release"))?;

                        let platform_suffix: &str = if cfg!(target_os = "windows") {
                            ".exe"
                        } else if cfg!(target_os = "macos") {
                            ".dmg"
                        } else {
                            ""
                        };

                        let host_arch = std::env::consts::ARCH;

                        let download_url = assets
                            .iter()
                            .find_map(|asset| {
                                if let Some(name) = asset["name"].as_str() {
                                    if name.contains("sdk_client")
                                        && name.contains(host_arch)
                                        && (platform_suffix.is_empty() || name.ends_with(platform_suffix))
                                    {
                                        return asset["browser_download_url"]
                                            .as_str()
                                            .map(|s| s.to_string());
                                    }
                                }
                                None
                            })
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "No sdk_client asset found for arch {} in release {}",
                                    host_arch, tag_name
                                )
                            })?;

                        return Ok((tag_name, download_url));
                    }

                    let headers = response.headers();
                    let ratelimit_remaining = headers
                        .get("x-ratelimit-remaining")
                        .and_then(|v| v.to_str().ok());
                    let retry_after = headers
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok());

                    warn!(
                        url = GITHUB_API_URL,
                        status = status.as_u16(),
                        ratelimit_remaining = ?ratelimit_remaining,
                        retry_after = ?retry_after,
                        attempt = attempt,
                        "Failed to fetch latest MystNodes release"
                    );

                    if status == reqwest::StatusCode::FORBIDDEN
                        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
                    {
                        last_error = Some(anyhow::anyhow!(
                            "GitHub API returned HTTP {} (x-ratelimit-remaining={:?}, retry-after={:?})",
                            status.as_u16(),
                            ratelimit_remaining,
                            retry_after
                        ));

                        if attempt < max_attempts {
                            let delay = base_delay * 2u32.pow(attempt - 1);
                            warn!(delay = ?delay, "Retrying GitHub API call after rate-limit backoff");
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                    } else {
                        return Err(anyhow::anyhow!("Failed to fetch latest release: HTTP {}", status.as_u16()));
                    }
                }
                Err(e) => {
                    warn!(error = %e, attempt = attempt, "GitHub API request error");
                    last_error = Some(anyhow::anyhow!("GitHub API request error: {}", e));
                    if attempt < max_attempts {
                        let delay = base_delay * 2u32.pow(attempt - 1);
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Failed to fetch latest release after all retries")))
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

        info!("Installing MystNodes sdk_client from GitHub latest release");

        let (version, download_url) = Self::fetch_latest_release().await?;
        info!(version = %version, download_url = %download_url, "Found latest release");

        let token = Self::github_token();
        download_file_with_options(&download_url, &binary, USER_AGENT, token.as_deref()).await?;

        #[cfg(not(target_os = "windows"))]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o755);
            std::fs::set_permissions(&binary, perms)?;
        }

        info!(binary = ?binary, version = %version, "MystNodes sdk_client installed successfully");
        Ok(())
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
