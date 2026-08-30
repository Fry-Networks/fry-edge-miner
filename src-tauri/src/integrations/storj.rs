use crate::supervisor::platform::BoundedOutput;
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

    /// Whether a storagenode dashboard answers successfully at `url`.
    ///
    /// Split out so the health-detection path can be exercised against a real
    /// HTTP responder. Reaching this state for real still requires a Storj
    /// account: `start()` launches nothing, and the dashboard only exists once
    /// the user has completed node identity with an account-issued auth token.
    async fn dashboard_responds(url: &str) -> bool {
        matches!(
            reqwest::Client::new()
                .get(url)
                .timeout(Duration::from_secs(5))
                .send()
                .await,
            Ok(resp) if resp.status().is_success()
        )
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

    /// The storagenode release ships several Windows amd64 zips —
    /// `storagenode_windows_amd64.zip`, `storagenode_msi_windows_amd64.zip` and
    /// `storagenode-updater_windows_amd64.zip`. A contains()-based match accepted
    /// all three and took whichever GitHub listed first, which is how the partner
    /// directory ended up holding only `storagenode-updater.exe` and start()
    /// failing with "storagenode binary not found". Match the one asset we want.
    fn is_storagenode_windows_asset(name: &str) -> bool {
        name == "storagenode_windows_amd64.zip"
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
                                let name = asset["name"].as_str()?;
                                if Self::is_storagenode_windows_asset(name) {
                                    return asset["browser_download_url"]
                                        .as_str()
                                        .map(|s| s.to_string());
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
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
                .map_err(|e| anyhow::anyhow!("Failed to extract ZIP: {}", e))?;
        }

        // Clean up ZIP
        let _ = std::fs::remove_file(&zip_path);

        // Some releases nest the payload one level down. Titan hits the same
        // shape and relocates; do it defensively here so a future layout change
        // does not resurrect the "binary not found" failure.
        if !binary.exists() {
            if let Ok(entries) = std::fs::read_dir(&extract_dir) {
                for entry in entries.flatten() {
                    let nested = entry.path().join(
                        binary.file_name().and_then(|n| n.to_str()).unwrap_or("storagenode.exe"),
                    );
                    if entry.path().is_dir() && nested.exists() {
                        std::fs::rename(&nested, &binary)?;
                        info!(from = ?nested, to = ?binary, "Relocated storagenode binary out of nested dir");
                        let _ = std::fs::remove_dir_all(entry.path());
                        break;
                    }
                }
            }
        }

        // install() used to return Ok even when nothing usable landed on disk,
        // so the real failure only surfaced later at start().
        if !binary.exists() {
            anyhow::bail!(
                "storagenode binary missing after extracting {} — expected it at {}",
                download_url,
                binary.display()
            );
        }

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
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT);
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = crate::supervisor::platform::command("killall")
                .arg("storagenode")
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT);
        }

        info!("Storj storagenode stopped");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        let binary = Self::binary_path();
        if !binary.exists() {
            return HealthStatus::Stopped;
        }
        // (probe extracted to StorjIntegration::dashboard_responds so the
        //  detection path can be exercised against a real HTTP responder)

        // Check if dashboard is accessible on localhost:14002
        match Self::dashboard_responds(&format!("http://127.0.0.1:{}", STORJ_DASHBOARD_PORT)).await {
            true => {
                HealthStatus::Healthy
            }
            false => {
                // TODO-COVERAGE-GAP: node-online state requires Storj account auth token (email signup).
                // Install + eligibility + exclusivity work without the token; this state guides the
                // user to paste their token for full activation. F4: the toggle "not staying on" is
                // this awaiting-setup state, not a crash — make the required action explicit so the
                // card reads as "needs your Storj token", not a failed start.
                HealthStatus::Unhealthy(
                    "Awaiting Storj setup — create a node auth token at storj.io and complete node identity to bring this node online. Install and eligibility are already active."
                        .to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim Windows-amd64 zip names from storj/storj v1.161.7.
    const RELEASE_ASSETS: &[&str] = &[
        "identity_windows_amd64.zip",
        "multinode_windows_amd64.zip",
        "storagenode-updater_windows_amd64.zip",
        "storagenode_msi_windows_amd64.zip",
        "storagenode_windows_amd64.zip",
    ];

    fn picked<'a>(names: &[&'a str]) -> Option<&'a str> {
        names
            .iter()
            .copied()
            .find(|n| StorjIntegration::is_storagenode_windows_asset(n))
    }

    #[test]
    fn picks_the_storagenode_zip_and_nothing_else() {
        assert_eq!(picked(RELEASE_ASSETS), Some("storagenode_windows_amd64.zip"));
    }

    #[test]
    fn never_picks_the_updater() {
        // This is the shipped bug: the updater sorted first and won, so the
        // partner directory ended up with storagenode-updater.exe only.
        assert!(!StorjIntegration::is_storagenode_windows_asset(
            "storagenode-updater_windows_amd64.zip"
        ));
    }

    #[test]
    fn never_picks_the_msi_bundle() {
        assert!(!StorjIntegration::is_storagenode_windows_asset(
            "storagenode_msi_windows_amd64.zip"
        ));
    }

    #[test]
    fn ignores_other_platforms_and_components() {
        for name in [
            "storagenode_linux_amd64.zip",
            "storagenode-modular_linux_amd64.zip",
            "identity_windows_amd64.zip",
            "multinode_windows_amd64.zip",
        ] {
            assert!(
                !StorjIntegration::is_storagenode_windows_asset(name),
                "should not have matched {name}"
            );
        }
    }

    #[test]
    fn none_when_only_an_updater_is_published() {
        assert_eq!(picked(&["storagenode-updater_windows_amd64.zip"]), None);
    }
}

#[cfg(test)]
mod dashboard_detection_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Serves a real HTTP 200 until the test drops it.
    fn spawn_dashboard() -> (u16, std::sync::mpsc::Sender<()>) {
        // Prefer the real dashboard port; fall back to an ephemeral one if the
        // environment will not allow binding it (another process, or policy).
        let listener = TcpListener::bind(("127.0.0.1", STORJ_DASHBOARD_PORT))
            .or_else(|_| TcpListener::bind(("127.0.0.1", 0)))
            .expect("bind a loopback listener");
        let port = listener.local_addr().unwrap().port();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                if let Ok(mut sock) = stream {
                    let mut buf = [0u8; 1024];
                    let _ = sock.read(&mut buf);
                    let _ = sock.write_all(
                        b"HTTP/1.1 200 OK
Content-Length: 2
Connection: close

ok",
                    );
                    let _ = sock.flush();
                }
            }
        });
        (port, stop_tx)
    }

    /// Binds a REAL HTTP responder and confirms the detection path reports
    /// success when a dashboard genuinely answers.
    ///
    /// This verifies FEM's health-DETECTION logic only. It is not evidence that
    /// Storj works: `start()` launches no storagenode, and a real node still
    /// requires an account-issued auth token and completed node identity.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_responding_dashboard_is_detected() {
        let (port, stop) = spawn_dashboard();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let detected =
            StorjIntegration::dashboard_responds(&format!("http://127.0.0.1:{port}")).await;
        let _ = stop.send(());
        assert!(detected, "a live HTTP 200 on the dashboard port must be detected");
    }

    /// Nothing listening must NOT be reported as a working dashboard — this is
    /// the state every device without a Storj account is actually in.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_dead_port_is_not_detected() {
        assert!(!StorjIntegration::dashboard_responds("http://127.0.0.1:1").await);
    }
}
