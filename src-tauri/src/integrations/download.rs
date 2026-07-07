use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{info, warn};

/// Get the base directory for FEM partner binaries
pub fn partners_base_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| {
            #[cfg(windows)]
            { PathBuf::from("C:/ProgramData") }
            #[cfg(not(windows))]
            { dirs::home_dir()
                .map(|h| h.join(".local").join("share"))
                .unwrap_or_else(|| PathBuf::from("/tmp")) }
        })
        .join("FryEdgeMiner")
        .join("partners")
}

/// Default User-Agent for all partner downloads.
pub const DEFAULT_USER_AGENT: &str =
    concat!("FryEdgeMiner/", env!("CARGO_PKG_VERSION"));

/// Download a file from URL to destination path with retry/backoff.
///
/// Retries up to `max_attempts` on HTTP 403/429 (GitHub rate-limit/abuse)
/// with exponential backoff starting at 2s.
pub async fn download_file(
    url: &str,
    dest: &Path,
) -> anyhow::Result<()> {
    download_file_with_options(url, dest, DEFAULT_USER_AGENT, None).await
}

/// Download a file with explicit User-Agent and optional Bearer auth token.
pub async fn download_file_with_options(
    url: &str,
    dest: &Path,
    user_agent: &str,
    auth_token: Option<&str>,
) -> anyhow::Result<()> {
    let client = build_client(user_agent, auth_token);
    let max_attempts = 3u32;
    let base_delay = Duration::from_secs(2);

    let mut last_error = None;

    for attempt in 1..=max_attempts {
        info!(url = url, dest = ?dest, attempt = attempt, "Downloading file");

        match client.get(url).send().await {
            Ok(response) => {
                let status = response.status();

                if status.is_success() {
                    // Stream to a .part file: large installers must not be
                    // buffered in RAM, and a mid-body failure ("error
                    // decoding response body") must stay INSIDE the retry
                    // loop instead of escaping via `?`.
                    if let Some(parent) = dest.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    let part_path = dest.with_extension("part");
                    match stream_to_file(response, &part_path).await {
                        Ok(bytes) => {
                            if dest.exists() {
                                std::fs::remove_file(dest)?;
                            }
                            std::fs::rename(&part_path, dest)?;
                            info!(dest = ?dest, bytes = bytes, "Download complete");
                            return Ok(());
                        }
                        Err(e) => {
                            let _ = std::fs::remove_file(&part_path);
                            warn!(url = url, error = %e, attempt = attempt, "Download body read failed");
                            last_error = Some(anyhow::anyhow!(
                                "Download of {} failed while reading the response body: {}",
                                url,
                                e
                            ));
                            if attempt < max_attempts {
                                let delay = base_delay * 2u32.pow(attempt - 1);
                                tokio::time::sleep(delay).await;
                            }
                            continue;
                        }
                    }
                }

                let headers = response.headers();
                let ratelimit_remaining = headers
                    .get("x-ratelimit-remaining")
                    .and_then(|v| v.to_str().ok());
                let retry_after = headers
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok());

                warn!(
                    url = url,
                    status = status.as_u16(),
                    ratelimit_remaining = ?ratelimit_remaining,
                    retry_after = ?retry_after,
                    attempt = attempt,
                    "Download request failed"
                );

                if status == reqwest::StatusCode::FORBIDDEN
                    || status == reqwest::StatusCode::TOO_MANY_REQUESTS
                {
                    last_error = Some(anyhow::anyhow!(
                        "Download failed: HTTP {} (x-ratelimit-remaining={:?}, retry-after={:?})",
                        status.as_u16(),
                        ratelimit_remaining,
                        retry_after
                    ));

                    if attempt < max_attempts {
                        let delay = base_delay * 2u32.pow(attempt - 1);
                        warn!(delay = ?delay, "Retrying download after rate-limit backoff");
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                } else {
                    return Err(anyhow::anyhow!("Download failed: HTTP {}", status.as_u16()));
                }
            }
            Err(e) => {
                warn!(error = %e, attempt = attempt, "Download request error");
                last_error = Some(anyhow::anyhow!("Download request error: {}", e));
                if attempt < max_attempts {
                    let delay = base_delay * 2u32.pow(attempt - 1);
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Download of {} failed after all retries", url)))
}

/// Write the response body to `dest` chunk by chunk, returning bytes written.
async fn stream_to_file(mut response: reqwest::Response, dest: &Path) -> anyhow::Result<u64> {
    use std::io::Write;

    let mut file = std::fs::File::create(dest)?;
    let mut total: u64 = 0;
    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk)?;
        total += chunk.len() as u64;
    }
    file.flush()?;
    Ok(total)
}

/// Build a reqwest client suitable for downloading public release assets.
fn build_client(user_agent: &str, auth_token: Option<&str>) -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(token) = auth_token {
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token))
                .expect("invalid auth token header value"),
        );
    }

    reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .user_agent(user_agent)
        .default_headers(headers)
        .build()
        .expect("failed to build HTTP client")
}
