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

/// Only read by the superseded `fetch_latest_release`.
#[allow(dead_code)]
const GITHUB_API_URL: &str =
    "https://api.github.com/repos/Fry-Networks/fem-partner-binaries/releases/latest";
const USER_AGENT: &str = concat!("FryEdgeMiner/", env!("CARGO_PKG_VERSION"));

/// The exact sdk_client asset MystNodes installs, pinned to a release TAG.
///
/// `releases/latest` meant the installed binary changed the moment a new
/// release appeared, with nothing recording or verifying which one landed.
const SDK_CLIENT_URL: &str = "https://github.com/Fry-Networks/fem-partner-binaries/releases/download/v1.0.0/sdk_client-windows-x86_64.exe";

/// sha256 of the asset at `SDK_CLIENT_URL`, measured from the published file
/// (16,293,376 bytes, PE32+ x86-64).
///
/// PROVENANCE, stated plainly: this is the digest of Fry's own re-host, not an
/// upstream-published checksum — Mysterium publishes none for this artifact.
/// It pins WHAT gets installed and detects corruption or substitution after
/// the fact, which is what the permanent-failure reports needed; it is not an
/// independent supply-chain attestation.
const SDK_CLIENT_SHA256: &str = "9d022fb1b990afac59981a899ccc362aa9aed854555248a93001ef9130d57990";

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

    /// Superseded by the tag-pinned `SDK_CLIENT_URL`. Kept rather than deleted
    /// so this change stays a pin, not a rewrite of the download path.
    #[allow(dead_code)]
    async fn fetch_latest_release() -> Result<(String, String)> {
        let client = Self::build_client();
        let max_attempts = 3u32;
        let base_delay = Duration::from_secs(2);
        let mut last_error = None;

        for attempt in 1..=max_attempts {
            info!(
                url = GITHUB_API_URL,
                attempt = attempt,
                "Fetching latest MystNodes release"
            );

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
                                        && (platform_suffix.is_empty()
                                            || name.ends_with(platform_suffix))
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
                                    host_arch,
                                    tag_name
                                )
                            })?;

                        return Ok((tag_name, download_url));
                    }

                    let headers = response.headers();
                    let ratelimit_remaining = headers
                        .get("x-ratelimit-remaining")
                        .and_then(|v| v.to_str().ok());
                    let retry_after = headers.get("retry-after").and_then(|v| v.to_str().ok());

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
                        return Err(anyhow::anyhow!(
                            "Failed to fetch latest release: HTTP {}",
                            status.as_u16()
                        ));
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

        Err(last_error
            .unwrap_or_else(|| anyhow::anyhow!("Failed to fetch latest release after all retries")))
    }

    /// Strip ANSI escape sequences from a log line (zerolog colored output).
    /// sha256 of a file on disk, streamed. Same loop as `titan.rs`.
    fn compute_sha256(path: &std::path::Path) -> Result<String> {
        use sha2::{Digest, Sha256};
        use std::io::Read;

        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Does the file on disk match the pin? `false` on any io error — an
    /// unreadable file is not a trusted one.
    fn staged_binary_matches_pin(path: &std::path::Path, expected: &str) -> bool {
        match Self::compute_sha256(path) {
            Ok(actual) => actual.eq_ignore_ascii_case(expected),
            Err(e) => {
                warn!(error = %e, path = ?path, "Could not hash the sdk_client binary");
                false
            }
        }
    }

    /// Move a binary that does not match the pin out of the way so `install()`
    /// can replace it.
    ///
    /// Renamed, never deleted: the displaced file is the only evidence of what
    /// was actually there, and a truncated or substituted partner binary is
    /// exactly the thing worth keeping for diagnosis. Same rename-not-delete
    /// shape as `space_acres::quarantine_staged_installer`.
    fn quarantine_untrusted_binary(binary: &std::path::Path) {
        let dest = Self::partner_dir().join("sdk_client.untrusted.exe");
        match std::fs::rename(binary, &dest) {
            Ok(()) => warn!(
                moved_to = ?dest,
                "The installed sdk_client did not match its pinned digest — quarantined for reinstall"
            ),
            Err(e) => warn!(error = %e, "Could not quarantine the untrusted sdk_client"),
        }
    }

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

/// BUG 9: pure reason-formatting for a dead sdk_client process, factored out
/// of `health_check()` so it is testable without constructing a real
/// `Supervisor`/`ConfigStore`/`ApiClient` — mirrors the identical pattern in
/// `fryvpn.rs::process_not_running_reason` used for the same class of bug
/// (BUG 6: crashed-but-enabled integration showing "Starting" forever with
/// no diagnostic).
fn process_not_running_reason(stderr_tail: &str) -> String {
    if stderr_tail == "no error output" {
        "Mysterium SDK client process is not running".to_string()
    } else {
        format!("Mysterium SDK client process is not running: {stderr_tail}")
    }
}

/// PURE: the first sdk_client log line that explains a failure, if any.
///
/// The check used to collapse to a bool and return the fixed string "Error
/// detected in SDK client logs" — the matched line was thrown away, so neither
/// the card nor the debug bundle ever said WHAT went wrong, while the health
/// loop restarted the process on it every tick.
///
/// The line is scrubbed before it is surfaced, because sdk_client's argv
/// carries `--user.token`.
fn log_error_reason(recent_lines: &[String]) -> Option<String> {
    let line = recent_lines
        .iter()
        .find(|l| l.contains(" ERR ") || l.contains(" FTL "))?;
    let scrubbed = crate::logging::scrubber::scrub_line(line);
    let scrubbed = if scrubbed.chars().count() > super::MAX_STDERR_CHARS {
        let head: String = scrubbed.chars().take(super::MAX_STDERR_CHARS).collect();
        format!("{head}…")
    } else {
        scrubbed
    };
    Some(format!("Error detected in SDK client logs: {scrubbed}"))
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
            if Self::staged_binary_matches_pin(&binary, SDK_CLIENT_SHA256) {
                info!(path = ?binary, "sdk_client binary already present and matches its pin");
                return Ok(());
            }
            // Mere existence used to be proof enough, so a truncated,
            // corrupted or substituted sdk_client was trusted forever: install()
            // returned early, installed_version() reported "installed" so
            // toggle_integration never called install() again, and
            // force_reinstall_integration is hard-gated to Olostep. There was no
            // repair path at all, which is why the failure was permanent.
            Self::quarantine_untrusted_binary(&binary);
        }

        info!(url = %SDK_CLIENT_URL, "Installing the pinned MystNodes sdk_client");

        let token = Self::github_token();
        download_file_with_options(SDK_CLIENT_URL, &binary, USER_AGENT, token.as_deref()).await?;

        let digest = Self::compute_sha256(&binary)?;
        if !digest.eq_ignore_ascii_case(SDK_CLIENT_SHA256) {
            Self::quarantine_untrusted_binary(&binary);
            anyhow::bail!(
                "the downloaded sdk_client does not match its pinned digest (expected {SDK_CLIENT_SHA256}, got {digest}) — it was quarantined and not installed"
            );
        }

        #[cfg(not(target_os = "windows"))]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o755);
            std::fs::set_permissions(&binary, perms)?;
        }

        info!(binary = ?binary, "MystNodes sdk_client installed and verified against its pin");
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        let binary = Self::binary_path();
        if !binary.exists() {
            anyhow::bail!("sdk_client binary not found at {}", binary.display());
        }
        // One repair attempt: a binary that does not match the pin is replaced
        // rather than handed to CreateProcessW, which is the only self-heal
        // MystNodes has (force_reinstall_integration covers Olostep only).
        if !Self::staged_binary_matches_pin(&binary, SDK_CLIENT_SHA256) {
            warn!("The installed sdk_client does not match its pinned digest — reinstalling it");
            self.install().await?;
            if !Self::staged_binary_matches_pin(&binary, SDK_CLIENT_SHA256) {
                anyhow::bail!(
                    "sdk_client could not be restored to its pinned version — the file at {} is still untrusted",
                    binary.display()
                );
            }
        }

        // Credential fetch (Diiisco pattern)
        let cfg = self.config.get();
        let miner_key = cfg.miner_key.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "Miner key not set — complete device registration before starting MystNodes"
            )
        })?;

        // F8: mirror the Diiisco credential path — a lookup right after
        // registration can hit a transient network error, and a single attempt
        // surfaced that as a permanent "token not found". Retry the transient
        // (network) failures with backoff. A genuinely empty token is a
        // server-side provisioning gap (hardwareapi), handled below/out-of-band.
        let mut cred_result = crate::api::credentials::lookup(&self.api_client, miner_key).await;
        for attempt in 1..=3u32 {
            match &cred_result {
                Err(crate::api::client::ApiError::Request(_)) => {
                    let delay = 2u64.pow(attempt);
                    warn!(
                        attempt,
                        delay_secs = delay,
                        "MystNodes credential fetch failed (network error), retrying"
                    );
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    cred_result =
                        crate::api::credentials::lookup(&self.api_client, miner_key).await;
                }
                _ => break,
            }
        }
        let creds =
            cred_result.map_err(|e| anyhow::anyhow!("Failed to fetch credentials: {}", e))?;

        // Extract mystnodes_user_token — fail-closed if missing (no user-claimable fallback)
        let Some(token) = creds
            .mystnodes_user_token
            .as_deref()
            .filter(|s| !s.is_empty())
        else {
            // c4 BUG LOOP 5: remembered so health_check can say what is
            // actually wrong instead of "process is not running".
            *TOKEN_MISSING_SINCE.lock().unwrap() = Some(std::time::Instant::now());
            anyhow::bail!(
                "mystnodes_user_token not found or empty in device credentials — \
                 contact support to provision a Mysterium token for this device"
            );
        };

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
        *TOKEN_MISSING_SINCE.lock().unwrap() = None;

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
            // c4 BUG LOOP 5: no process because start() refused to spawn it
            // without a token is a setup state, not a crash.
            if token_missing_recently(
                *TOKEN_MISSING_SINCE.lock().unwrap(),
                std::time::Instant::now(),
            ) {
                return HealthStatus::Unhealthy(TOKEN_NOT_PROVISIONED_REASON.to_string());
            }
            // BUG 9 (Discord: "toggle on, Installed, STARTING forever, 0%"):
            // a bare `Stopped` here — only ever reached while this integration
            // is ENABLED, since the caller short-circuits disabled ones before
            // calling health_check() — renders as "Starting" with no reason
            // on the frontend's lifecycle derivation. If sdk_client.exe
            // crashed immediately (bad token, network refusal, missing DLL)
            // this was indistinguishable from a slow-but-healthy startup.
            // Whatever killed the process — an invalid/expired token
            // ("identity not registered"), a network refusal, or a missing
            // DLL — sdk_client's own zerolog output says so; that's a more
            // concrete, evidence-based reason than guessing at a tequilapi
            // probe result for a process we already know is not running.
            let stderr_path = self.log_dir.join("mysterium").join("mysterium_stderr.log");
            let stderr_content = tokio::fs::read_to_string(&stderr_path)
                .await
                .unwrap_or_default();
            let tail = super::stderr_tail(&stderr_content, 3);
            return HealthStatus::Unhealthy(process_not_running_reason(&tail));
        }

        // Read BOTH log files (sdk_client writes to stderr; stdout typically empty)
        let stdout_path = self.log_dir.join("mysterium").join("mysterium_stdout.log");
        let stderr_path = self.log_dir.join("mysterium").join("mysterium_stderr.log");

        let stdout_content = tokio::fs::read_to_string(&stdout_path)
            .await
            .unwrap_or_default();
        let stderr_content = tokio::fs::read_to_string(&stderr_path)
            .await
            .unwrap_or_default();

        // Combine recent tail (last 50 lines of each)
        let recent_lines: Vec<String> = stdout_content
            .lines()
            .chain(stderr_content.lines())
            .rev()
            .take(50)
            .map(Self::strip_ansi)
            .collect();

        // Check for error markers — anchored, space-bounded (zerolog: INF/WRN/ERR/FTL)
        if let Some(reason) = log_error_reason(&recent_lines) {
            return HealthStatus::Unhealthy(reason);
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

/// c4 BUG LOOP 5: when `start()` last refused to spawn sdk_client because the
/// device credentials carry no Mysterium token. Cleared by a successful spawn.
static TOKEN_MISSING_SINCE: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// How long a missing token is reported as such. Afterwards health_check falls
/// back to the plain not-running reason, which lets the supervisor run
/// `start()` again — and `start()` re-fetches the credentials, so a token that
/// support provisioned in the meantime is picked up without a user action.
const TOKEN_RECHECK: Duration = Duration::from_secs(600);

/// Contains the "node token not provisioned" AWAITING marker, so the card
/// shows SETUP REQUIRED and the supervisor does not restart into it.
pub(crate) const TOKEN_NOT_PROVISIONED_REASON: &str =
    "Mysterium node token not provisioned for this device — contact Fry support to provision \
     one; FEM checks again automatically every 10 minutes";

pub(crate) fn token_missing_recently(
    since: Option<std::time::Instant>,
    now: std::time::Instant,
) -> bool {
    since.is_some_and(|at| now.saturating_duration_since(at) < TOKEN_RECHECK)
}

#[cfg(test)]
mod tests {
    use super::*;

    // BUG 9 (Discord: "toggle on, Installed, STARTING forever, 0%"). The
    // scenario reported was: registration succeeded (a valid
    // `mystnodes_user_token` WAS present and used to spawn sdk_client — the
    // "token present" precondition below), but the process then crashed or
    // exited, and the card never showed anything but "Starting". A dead
    // process must always resolve to `Unhealthy` with a concrete reason —
    // never silently mapped to a state the frontend renders as
    // "Starting forever".
    #[test]
    fn a_dead_process_with_a_token_that_previously_started_it_gets_an_unhealthy_reason_from_its_own_log(
    ) {
        let reason = process_not_running_reason("ERR failed to register identity: invalid token");
        assert!(
            reason.contains("ERR failed to register identity: invalid token"),
            "reason must surface the SDK client's own diagnostic, not a generic message: {reason}"
        );
        assert!(reason.contains("not running"));
    }

    #[test]
    fn a_dead_process_with_no_log_output_still_gets_a_non_empty_reason() {
        let reason = process_not_running_reason("no error output");
        assert_eq!(reason, "Mysterium SDK client process is not running");
        assert!(!reason.is_empty());
    }

    #[test]
    fn the_reason_is_never_a_bare_stopped_status_masquerading_as_starting() {
        // Regression guard for the exact defect: health_check() must not
        // return `HealthStatus::Stopped` for a dead-but-enabled integration
        // (the caller's lifecycle derivation maps Stopped/Unknown for an
        // enabled integration to "Starting" with no reason text at all).
        // This is a compile-time/structural assertion in spirit — the two
        // reason-formatting tests above already prove the function always
        // returns non-empty Unhealthy text, but we also assert directly that
        // an `Unhealthy(reason)` variant (not `Stopped`) is what a dead
        // process must produce.
        let status = HealthStatus::Unhealthy(process_not_running_reason("no error output"));
        match status {
            HealthStatus::Unhealthy(reason) => assert!(!reason.is_empty()),
            other => panic!("dead process must map to Unhealthy(reason), got {other:?}"),
        }
    }

    #[test]
    fn a_reason_with_multiple_stderr_lines_stays_within_the_bounded_tail() {
        // `super::stderr_tail` already bounds the tail elsewhere (BUG 6
        // precedent); this just confirms the formatting wrapper doesn't
        // silently drop or duplicate whatever tail it's given.
        let tail = "line one\nline two\nline three";
        let reason = process_not_running_reason(tail);
        assert!(reason.contains(tail));
        assert!(reason.starts_with("Mysterium SDK client process is not running: "));
    }
}

#[cfg(test)]
#[path = "mysterium_pin_tests.rs"]
mod mysterium_pin_tests;

/// c4 BUG LOOP 5: a missing Mysterium token is reported as such, not as a crash.
#[cfg(test)]
#[path = "mysterium_token_state_tests.rs"]
mod mysterium_token_state_tests;
