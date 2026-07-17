use super::download::{download_file_with_options, partners_base_dir};
use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{info, warn};

const GITHUB_API_URL: &str = "https://api.github.com/repos/storj/storj/releases/latest";
const USER_AGENT: &str = concat!("FryEdgeMiner/", env!("CARGO_PKG_VERSION"));
const STORJ_DASHBOARD_PORT: u16 = 14002;

pub struct StorjIntegration;

impl StorjIntegration {
    fn partner_dir() -> PathBuf {
        partners_base_dir().join("storj")
    }

    fn binary_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        return Self::partner_dir().join("storagenode.exe");
        #[cfg(not(target_os = "windows"))]
        return Self::partner_dir().join("storagenode");
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
            info!(url = GITHUB_API_URL, attempt = attempt, "Fetching latest Storj release");

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

                        let download_url = assets
                            .iter()
                            .find_map(|asset| {
                                if let Some(name) = asset["name"].as_str() {
                                    if name.contains("storagenode")
                                        && name.contains("windows")
                                        && name.contains("amd64")
                                        && name.ends_with(".zip")
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
                                    "No storagenode windows amd64 asset found in release {}",
                                    tag_name
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
                        "Failed to fetch latest Storj release"
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
}

#[async_trait]
impl Integration for StorjIntegration {
    fn id(&self) -> &str {
        "storj"
    }

    fn display_name(&self) -> &str {
        "Storj"
    }

    async fn install(&self) -> Result<()> {
        let binary = Self::binary_path();
        if binary.exists() {
            info!(path = ?binary, "Storj storagenode binary already present");
            return Ok(());
        }

        info!("Installing Storj storagenode from GitHub latest release");

        let (version, download_url) = Self::fetch_latest_release().await?;
        info!(version = %version, download_url = %download_url, "Found latest release");

        let token = Self::github_token();
        let zip_path = Self::partner_dir().join("storagenode.zip");
        download_file_with_options(&download_url, &zip_path, USER_AGENT, token.as_deref()).await?;

        // Extract the ZIP file
        let extract_dir = Self::partner_dir();
        std::fs::create_dir_all(&extract_dir)?;

        #[cfg(target_os = "windows")]
        {
            use std::process::Command;
            Command::new("powershell")
                .args([
                    "-Command",
                    &format!(
                        "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                        zip_path.display(),
                        extract_dir.display()
                    ),
                ])
                .output()
                .map_err(|e| anyhow::anyhow!("Failed to extract ZIP: {}", e))?;
        }

        // Clean up ZIP
        let _ = std::fs::remove_file(&zip_path);

        info!(binary = ?binary, version = %version, "Storj storagenode installed successfully");
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        let binary = Self::binary_path();
        if !binary.exists() {
            anyhow::bail!("storagenode binary not found at {}", binary.display());
        }

        info!("Storj storagenode node activation requires manual auth token setup");
        info!("Binary installed at: {}", binary.display());
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            let _ = crate::supervisor::platform::command("taskkill")
                .args(["/IM", "storagenode.exe", "/F"])
                .output();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = crate::supervisor::platform::command("killall")
                .arg("storagenode")
                .output();
        }

        info!("Storj storagenode stopped");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        let binary = Self::binary_path();
        if !binary.exists() {
            return HealthStatus::Stopped;
        }

        // Check if dashboard is accessible on localhost:14002
        match reqwest::Client::new()
            .get(format!("http://127.0.0.1:{}", STORJ_DASHBOARD_PORT))
            .timeout(Duration::from_secs(5))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                HealthStatus::Healthy
            }
            _ => {
                // TODO-COVERAGE-GAP: node-online state requires Storj account auth token (email signup).
                // Install + eligibility + exclusivity work without the token; this state guides the
                // user to paste their token for full activation.
                HealthStatus::Unhealthy(
                    "Identity pending — complete Storj account setup at storj.io to activate this node".to_string()
                )
            }
        }
    }

    async fn check_update(&self) -> Result<Option<String>> {
        // TODO: partner binary version check via /versions/storj
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
        // Storj requires explicit token auth — no automatic eligibility.
        PocGateData {
            poa: false,
            ..Default::default()
        }
    }
}
