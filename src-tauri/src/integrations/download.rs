use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{info, warn};

/// BUG 1/4: the storage root, resolved ONCE at startup and never mutated.
///
/// `OnceLock`, deliberately not `RwLock`: if the root could flip mid-run,
/// `installed_version()` would start returning `None` and
/// `commands/integration.rs` would reinstall on top of a live partner, and
/// Iagon's node token would vanish from under a running node. A change
/// therefore takes effect on the next launch.
static STORAGE_ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// The historic location — verbatim the pre-fix body of `partners_base_dir`.
pub fn default_partners_base_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| {
            #[cfg(windows)]
            {
                PathBuf::from("C:/ProgramData")
            }
            #[cfg(not(windows))]
            {
                dirs::home_dir()
                    .map(|h| h.join(".local").join("share"))
                    .unwrap_or_else(|| PathBuf::from("/tmp"))
            }
        })
        .join("FryEdgeMiner")
        .join("partners")
}

/// PURE: the whole override decision, testable with no disk and no globals.
///
/// An unset / blank / whitespace-only value all mean "use the historic
/// location", so every existing install is byte-for-byte unaffected.
pub fn resolve_partners_base_dir(configured: Option<&str>, default: PathBuf) -> PathBuf {
    match configured.map(str::trim).filter(|s| !s.is_empty()) {
        Some(p) => crate::storage_location::storage_root_for(p),
        None => default,
    }
}

/// Called EXACTLY once, from `main.rs` setup, BEFORE the registry is built.
/// Validates; on any problem logs and falls back to the historic location
/// rather than bricking the app. Returns the root actually chosen.
pub fn init_storage_root(configured: Option<&str>) -> PathBuf {
    let default = default_partners_base_dir();
    let chosen = resolve_partners_base_dir(configured, default.clone());
    let chosen = if chosen != default {
        match crate::storage_location::probe_writable(&chosen) {
            Ok(()) => chosen,
            Err(e) => {
                warn!(
                    path = ?chosen,
                    error = %e.message(),
                    "Configured storage location is unusable — falling back to the default"
                );
                default
            }
        }
    } else {
        chosen
    };
    let _ = STORAGE_ROOT.set(chosen.clone());
    info!(path = ?chosen, "Storage root resolved");
    chosen
}

/// Get the base directory for FEM partner binaries.
///
/// Signature and name unchanged — which is why none of the 12 call sites
/// across 8 integration files needed editing. `unwrap_or_else` (not
/// `get_or_init`) means any path that runs before `init_storage_root`,
/// including every unit test, gets exactly the historic behaviour.
pub fn partners_base_dir() -> PathBuf {
    STORAGE_ROOT
        .get()
        .cloned()
        .unwrap_or_else(default_partners_base_dir)
}

/// PURE: pick between an integration's legacy deploy directory and the one
/// under the configured storage root.
///
/// Three partner deploy roots resolved `dirs::data_local_dir()` themselves and
/// never consulted the storage root at all, so moving storage left them behind
/// — which is half of "Titan broken on a custom storage root". Grandfathering
/// an existing legacy directory means no live deployment moves and no
/// migration is needed; only a fresh install lands under the configured root.
pub fn resolve_deploy_dir(legacy: PathBuf, rooted: PathBuf) -> PathBuf {
    if legacy.exists() && !rooted.exists() {
        legacy
    } else {
        rooted
    }
}

/// Default User-Agent for all partner downloads.
pub const DEFAULT_USER_AGENT: &str = concat!("FryEdgeMiner/", env!("CARGO_PKG_VERSION"));

/// Download a file from URL to destination path with retry/backoff.
///
/// Retries up to `max_attempts` on HTTP 403/429 (GitHub rate-limit/abuse)
/// with exponential backoff starting at 2s.
pub async fn download_file(url: &str, dest: &Path) -> anyhow::Result<()> {
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
                let retry_after = headers.get("retry-after").and_then(|v| v.to_str().ok());

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

    Err(last_error
        .unwrap_or_else(|| anyhow::anyhow!("Download of {} failed after all retries", url)))
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

/// BUG 1/4 (1337, georgeparis): "FEM only shows C: drive storage even when
/// installed on D:" and "Iagon says it requires 900 GB but only checks C:
/// (167 GB) instead of D: (2 TB)".
///
/// Root cause: `partners_base_dir()` was anchored to `dirs::data_dir()` =
/// `%APPDATA%`, which is always on the user-profile drive. Every storage
/// integration sizes and writes through this one function, so the drive FEM
/// probed could never be the drive the user wanted.
#[cfg(test)]
mod bug1_storage_root_tests {
    use super::*;

    /// THE headline regression test: the drive the free-space probe reads must
    /// follow the configured path.
    #[cfg(windows)]
    #[test]
    fn the_probed_drive_follows_the_configured_path() {
        let legacy = PathBuf::from(r"C:\Users\u\AppData\Roaming\FryEdgeMiner\partners");
        assert_eq!(
            crate::system_info::drive_letter(&legacy),
            Some("C".to_string()),
            "fixture must reproduce the bug: %APPDATA% is always on C:"
        );

        let chosen = resolve_partners_base_dir(Some(r"D:\FryEdgeMiner"), legacy.clone());
        assert_eq!(
            crate::system_info::drive_letter(&chosen),
            Some("D".to_string()),
            "BUG 1/4: the free-space probe must read the drive the user chose, not %APPDATA%'s C:"
        );
    }

    /// Upgrade safety: every existing install has no `storage_dir`, and must
    /// keep the byte-for-byte historic path.
    #[test]
    fn an_unset_storage_dir_is_byte_for_byte_the_legacy_path() {
        let legacy = default_partners_base_dir();
        for configured in [None, Some(""), Some("   ")] {
            assert_eq!(
                resolve_partners_base_dir(configured, legacy.clone()),
                legacy,
                "configured={configured:?} must mean the historic location"
            );
        }
    }

    /// The fixed leaf is a SAFETY boundary: `aem.rs` force-clean does a
    /// `remove_dir_all(partners_base_dir().join("aem"))`, so a bare `D:\`
    /// must never become the root.
    #[test]
    fn a_configured_root_never_lets_force_clean_escape_to_the_drive_root() {
        let root = resolve_partners_base_dir(Some(r"D:\"), default_partners_base_dir());
        assert_eq!(root, PathBuf::from(r"D:\FryEdgeMiner\partners"));
        let aem = root.join("aem");
        assert_ne!(
            aem,
            PathBuf::from(r"D:\aem"),
            "force-clean would target the drive root"
        );
    }

    /// B13: three partner deploy roots never consulted the storage root, so a
    /// user who moved storage left them stranded. Grandfathering an existing
    /// legacy directory is what makes the fix migration-free.
    #[test]
    fn an_existing_legacy_deploy_dir_is_grandfathered_so_no_live_deployment_moves() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join("legacy").join("sentinel");
        let rooted = tmp.path().join("rooted").join("sentinel");
        std::fs::create_dir_all(&legacy).unwrap();

        assert_eq!(resolve_deploy_dir(legacy.clone(), rooted), legacy);
    }

    #[test]
    fn a_fresh_deploy_lands_under_the_configured_storage_root() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join("legacy").join("sentinel");
        let rooted = tmp.path().join("rooted").join("sentinel");

        assert_eq!(resolve_deploy_dir(legacy, rooted.clone()), rooted);
    }

    /// Once the rooted directory exists it wins, even if the legacy one is
    /// still lying around — otherwise a deployment would flip back and forth.
    #[test]
    fn a_deployment_already_under_the_root_is_not_dragged_back_to_the_legacy_path() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join("legacy").join("sentinel");
        let rooted = tmp.path().join("rooted").join("sentinel");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&rooted).unwrap();

        assert_eq!(resolve_deploy_dir(legacy, rooted.clone()), rooted);
    }
}
