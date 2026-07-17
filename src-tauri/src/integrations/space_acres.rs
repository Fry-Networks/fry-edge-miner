use super::download::{download_file_with_options, partners_base_dir};
use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{info, warn};

const SPACE_ACRES_MIN_GB: u64 = 50;

pub struct SpaceAcresIntegration;

const GITHUB_API_URL: &str = "https://api.github.com/repos/autonomys/space-acres/releases/latest";
const USER_AGENT: &str = concat!("FryEdgeMiner/", env!("CARGO_PKG_VERSION"));

impl SpaceAcresIntegration {
    fn partner_dir() -> PathBuf {
        partners_base_dir().join("space_acres")
    }

    fn binary_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        return Self::partner_dir().join("space-acres.exe");
        #[cfg(not(target_os = "windows"))]
        return Self::partner_dir().join("space-acres");
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
            info!(url = GITHUB_API_URL, attempt = attempt, "Fetching latest SpaceAcres release");

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
                            ".AppImage"
                        };

                        let host_arch = std::env::consts::ARCH; // "x86_64", "aarch64", etc.

                        let download_url = assets
                            .iter()
                            .find_map(|asset| {
                                if let Some(name) = asset["name"].as_str() {
                                    if name.ends_with(platform_suffix) && name.contains(host_arch) {
                                        return asset["browser_download_url"]
                                            .as_str()
                                            .map(|s| s.to_string());
                                    }
                                }
                                None
                            })
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "No {} asset found for arch {} in release",
                                    platform_suffix, host_arch
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
                        "Failed to fetch latest SpaceAcres release"
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

    fn is_running() -> bool {
        #[cfg(target_os = "windows")]
        {
            crate::supervisor::platform::command("tasklist")
                .output()
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .to_lowercase()
                        .contains("space-acres.exe")
                })
                .unwrap_or(false)
        }
        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    }

    /// Check if system meets SpaceAcres eligibility requirements:
    /// - System has an SSD (or the SeekPenalty=false fallback indicates one)
    /// - Free disk space >= SPACE_ACRES_MIN_GB
    /// Returns (eligible, Option<reason>) — if ineligible, reason explains why.
    pub async fn check_eligibility() -> (bool, Option<String>) {
        if !has_ssd() {
            return (false, Some("No SSD detected — SpaceAcres requires solid-state storage".to_string()));
        }

        // Check free disk space on the partners base directory
        match check_free_space().await {
            Ok(free_gb) => {
                if free_gb < SPACE_ACRES_MIN_GB {
                    return (
                        false,
                        Some(format!(
                            "Insufficient disk space — {} GB free, {} GB required",
                            free_gb, SPACE_ACRES_MIN_GB
                        )),
                    );
                }
                (true, None)
            }
            Err(e) => {
                warn!(error = %e, "Failed to check disk space");
                // TODO-verify: SpaceAcres actual minimum
                // Assume eligible if check fails (don't block on uncertain state)
                (true, None)
            }
        }
    }
}

#[async_trait]
impl Integration for SpaceAcresIntegration {
    fn id(&self) -> &str {
        "space_acres"
    }

    fn display_name(&self) -> &str {
        "SpaceAcres"
    }

    async fn install(&self) -> Result<()> {
        let binary = Self::binary_path();

        // Check if binary already exists
        if binary.exists() {
            info!(path = ?binary, "SpaceAcres binary already installed");
            return Ok(());
        }

        info!("Installing SpaceAcres from GitHub latest release");

        // Fetch latest release metadata
        let (version, download_url) = Self::fetch_latest_release().await?;
        info!(version = %version, download_url = %download_url, "Found latest release");

        // Download the binary with retry/backoff and optional auth
        let token = Self::github_token();
        download_file_with_options(&download_url, &binary, USER_AGENT, token.as_deref()).await?;

        // Make it executable on Unix-like systems
        #[cfg(not(target_os = "windows"))]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o755);
            std::fs::set_permissions(&binary, perms)?;
        }

        info!(binary = ?binary, version = %version, "SpaceAcres installed successfully");
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        let binary = Self::binary_path();

        if !binary.exists() {
            anyhow::bail!("SpaceAcres binary not found at {:?}", binary);
        }

        if Self::is_running() {
            info!("SpaceAcres already running");
            return Ok(());
        }

        info!(binary = ?binary, "Starting SpaceAcres");

        // Spawn the process with a base directory argument
        let base_dir = Self::partner_dir().join("data");
        std::fs::create_dir_all(&base_dir)?;

        let _ = crate::supervisor::platform::command(&binary)
            .arg("--base-directory")
            .arg(&base_dir)
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!("Failed to start SpaceAcres: {}", e)
            })?;

        // Give it a moment to start
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        // Kill any running space-acres process
        #[cfg(target_os = "windows")]
        {
            let _ = crate::supervisor::platform::command("taskkill")
                .args(["/IM", "space-acres.exe", "/F"])
                .output();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = crate::supervisor::platform::command("killall")
                .arg("space-acres")
                .output();
        }

        info!("Stopped SpaceAcres");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        let binary = Self::binary_path();
        if !binary.exists() {
            return HealthStatus::Stopped;
        }
        if !has_ssd() {
            warn!("No SSD detected — SpaceAcres performance will be degraded");
            return HealthStatus::Unhealthy(
                "No SSD detected — SpaceAcres performance degraded".to_string(),
            );
        }
        if Self::is_running() {
            HealthStatus::Healthy
        } else {
            HealthStatus::Stopped
        }
    }

    async fn check_update(&self) -> Result<Option<String>> {
        match Self::fetch_latest_release().await {
            Ok((version, _)) => {
                info!(version = %version, "Found SpaceAcres update available");
                Ok(Some(version))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to check SpaceAcres updates");
                Ok(None)
            }
        }
    }

    async fn apply_update(&self, version: &str) -> Result<()> {
        info!(version = %version, "Applying SpaceAcres update");
        // Stop the current instance
        self.stop().await?;
        // Backup old binary
        let binary = Self::binary_path();
        if binary.exists() {
            let backup = binary.with_extension("exe.bak");
            std::fs::copy(&binary, &backup)?;
        }
        // Re-run install which will download the latest
        self.install().await?;
        Ok(())
    }

    fn installed_version(&self) -> Option<String> {
        if Self::binary_path().exists() {
            Some("installed".into())
        } else {
            None
        }
    }

    fn collect_poc_data(&self) -> PocGateData {
        PocGateData {
            poa: Self::is_running(),
            ..Default::default()
        }
    }
}

/// Check available disk space on the partners directory.
/// Returns available space in GB.
async fn check_free_space() -> anyhow::Result<u64> {
    let base_dir = partners_base_dir();
    #[cfg(target_os = "windows")]
    {
        let path = base_dir.to_string_lossy().to_string();
        let drive = path.split(':').next().unwrap_or("C");

        let output = crate::supervisor::platform::command("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "((Get-Volume -DriveLetter {} | Select-Object -Expand SizeRemaining) / 1GB) -as [int64]",
                    drive
                ),
            ])
            .output()?;

        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u64>()
            .map_err(|e| anyhow::anyhow!("Failed to parse disk space: {}", e))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let output = crate::supervisor::platform::command("df")
            .arg("-BG")
            .arg(&base_dir)
            .output()?;

        // df output format: Filesystem 1G-blocks Used Available Use% Mounted on
        let lines: Vec<&str> = String::from_utf8_lossy(&output.stdout).lines().collect();
        if lines.len() > 1 {
            let parts: Vec<&str> = lines[1].split_whitespace().collect();
            if parts.len() > 3 {
                return parts[3]
                    .trim_end_matches('G')
                    .parse::<u64>()
                    .map_err(|e| anyhow::anyhow!("Failed to parse disk space: {}", e));
            }
        }
        anyhow::bail!("Failed to parse df output")
    }
}

/// Detect if system has an SSD.
/// Cached: the probe spawns a full PowerShell process (~1-3s) and this is
/// called from the 30s health-check loop — uncached it burns CPU forever,
/// and physical disks don't change while the app runs.
///
/// Primary: Get-PhysicalDisk | Where MediaType -eq 'SSD'
/// Fallback: MSFT_PhysicalDisk with SeekPenalty==false (indicates SSD)
#[cfg(target_os = "windows")]
fn has_ssd() -> bool {
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| {
        // Primary check: explicit SSD MediaType
        let primary_check = crate::supervisor::platform::command("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-PhysicalDisk | Where MediaType -eq 'SSD' | Measure-Object | Select -Expand Count",
            ])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<u32>()
                    .unwrap_or(0)
                    > 0
            })
            .unwrap_or(false);

        if primary_check {
            return true;
        }

        // Fallback: MSFT_PhysicalDisk with SeekPenalty==false indicates SSD
        crate::supervisor::platform::command("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-WmiObject -Namespace \"root/Microsoft/Windows/Storage\" -Class MSFT_PhysicalDisk | Where-Object SeekPenalty -EQ $false | Measure-Object).Count -gt 0",
            ])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .to_lowercase()
                    .contains("true")
            })
            .unwrap_or(false)
    })
}

#[cfg(not(target_os = "windows"))]
fn has_ssd() -> bool {
    false // Platform-specific SSD detection deferred
}
