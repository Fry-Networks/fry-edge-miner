use super::{HealthStatus, Integration, PocGateData};
use crate::config::store::ConfigStore;
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{info, warn};

const ALGOD_SERVER: &str = "https://mainnet-api.algonode.cloud";
const FRYNODE_VERSION: &str = "0.1.0";

/// BUG 6: dedicated Windows Firewall rule name for frynode.exe, same pattern
/// as `firewall::OLOSTEP_RULE_NAME`.
pub const FRYNODE_RULE_NAME: &str = "FEM-FryNode";

pub struct FryVpnIntegration {
    pub config: Arc<ConfigStore>,
    /// BUG 6: needed to fetch this device's provisioned Algorand credentials
    /// (`GET /credentials/{miner_key}`) so frynode registers with a FUNDED
    /// account instead of generating a fresh 0-ALGO one. Same field Diiisco
    /// and Mysterium already carry.
    pub api_client: Arc<crate::api::client::ApiClient>,
    pub supervisor: Arc<Mutex<crate::supervisor::Supervisor>>,
    /// BUG 6: base log directory (same one the Supervisor writes
    /// `<log_dir>/fryvpn/fryvpn_stderr.log` under) — needed so a crashed
    /// process can report why instead of a bare "Stopped".
    pub log_dir: PathBuf,
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

/// BUG 6: reason shown when frynode's process is not running, instead of a
/// bare `HealthStatus::Stopped` (which the frontend's lifecycle derivation
/// renders as "Starting" for an ENABLED integration — this is only ever
/// called when the integration is enabled, so a not-running process here
/// means it crashed or never started, not that it was intentionally
/// stopped). Pure so it is testable without touching the filesystem; the
/// caller passes an already-bounded stderr tail (or an empty string when
/// there was nothing to read).
fn process_not_running_reason(stderr_tail: &str) -> String {
    if stderr_tail.trim().is_empty() {
        "frynode process is not running".to_string()
    } else {
        format!("frynode process is not running: {stderr_tail}")
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

/// Minimum SPENDABLE balance (microAlgos) fryDVPN needs to register on-chain:
/// the registry app call plus its fee. 0.1 ALGO is comfortably above the
/// 1000 microAlgo minimum fee and any box/opt-in cost.
pub(crate) const REGISTRATION_MIN_MICROALGOS: u64 = 100_000;

/// PURE: what an account can actually spend — algod reports the TOTAL `amount`
/// and separately the locked `min-balance`. Saturating, so an account below its
/// own minimum reports 0 rather than underflowing.
pub(crate) fn spendable_microalgos(amount: u64, min_balance: u64) -> u64 {
    amount.saturating_sub(min_balance)
}

/// PURE: can this wallet afford on-chain registration?
///
/// Returns a message the device owner can ACT on. Deliberately never surfaces
/// the raw "overspend" the chain returns — that told minerman nothing.
pub(crate) fn registration_affordability(
    spendable: u64,
    required: u64,
    address: &str,
) -> Result<(), String> {
    if spendable >= required {
        return Ok(());
    }
    let short = (required - spendable) as f64 / 1_000_000.0;
    let have = spendable as f64 / 1_000_000.0;
    Err(format!(
        "fryDVPN needs about {:.3} ALGO in this device's wallet to register on-chain, and it          currently has {:.3} ALGO — about {:.3} ALGO short. Send ALGO to {} and fryDVPN will          register automatically on the next check.",
        required as f64 / 1_000_000.0,
        have,
        short,
        address
    ))
}

/// PURE: pull `amount` and `min-balance` out of an algod account response.
/// `None` on anything unparseable — an HTML error page must NEVER read as a
/// zero balance, or the pre-check would block a perfectly funded wallet.
pub(crate) fn parse_account_balance(body: &str) -> Option<(u64, u64)> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let amount = v.get("amount")?.as_u64()?;
    let min_balance = v.get("min-balance").and_then(|m| m.as_u64()).unwrap_or(0);
    Some((amount, min_balance))
}

impl FryVpnIntegration {
    /// Fetch the device's provisioned Algorand identity and confirm it can
    /// actually afford the on-chain registration (BUG 6).
    ///
    /// Returns the mnemonic to hand frynode. Any failure is a user-facing
    /// sentence, never a raw chain error.
    async fn resolve_funded_identity(&self) -> Result<String, String> {
        let miner_key = self
            .config
            .get()
            .miner_key
            .ok_or_else(|| "This device is not registered yet — finish setup first.".to_string())?;

        let creds = crate::api::credentials::lookup(&self.api_client, &miner_key)
            .await
            .map_err(|e| format!("Could not fetch this device's wallet: {e}"))?;

        let address = creds
            .algo_address
            .filter(|a| !a.trim().is_empty())
            .ok_or_else(|| {
                "Device wallet not provisioned yet — fryDVPN will start automatically once it is."
                    .to_string()
            })?;
        let mnemonic = creds.algo_mnemonic.filter(|m| !m.trim().is_empty()).ok_or_else(|| {
            "Device wallet key not available yet — fryDVPN will start automatically once it is."
                .to_string()
        })?;

        // Pre-check the balance so a 0-ALGO wallet never reaches the chain and
        // comes back as an opaque "overspend".
        let url = format!("{}/v2/accounts/{}", ALGOD_SERVER, address);
        let body = reqwest::Client::new()
            .get(&url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| format!("Could not check the fryDVPN wallet balance: {e}"))?
            .text()
            .await
            .map_err(|e| format!("Could not read the fryDVPN wallet balance: {e}"))?;

        match parse_account_balance(&body) {
            Some((amount, min_balance)) => {
                let spendable = spendable_microalgos(amount, min_balance);
                registration_affordability(spendable, REGISTRATION_MIN_MICROALGOS, &address)?;
            }
            None => {
                // Unreadable is NOT zero. Fail open rather than block a funded
                // wallet because algod returned an error page.
                warn!("Could not parse the algod account response — proceeding without a balance pre-check");
            }
        }
        Ok(mnemonic)
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

        // BUG 6: pre-create firewall rules for this exact binary path so
        // Windows never shows the firewall prompt at all (georgeparis 8/28
        // worked around this by hand with `New-NetFirewallRule`). Non-fatal:
        // a declined UAC just means Windows prompts as before. Only when the
        // resolved path is absolute — a bare PATH-lookup name has nothing
        // concrete to bind the rule to.
        let binary_path = std::path::Path::new(&binary);
        if binary_path.is_absolute() {
            if let Err(e) = super::firewall::ensure_program_rules(FRYNODE_RULE_NAME, binary_path) {
                warn!(error = %e, "Fry dVPN firewall rule setup failed — continuing");
            }
        }

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

        // BUG 6: hand frynode the device's OWN funded account. Without this it
        // generated a fresh 0-ALGO identity and RegisterNode failed with an
        // overspend the user could do nothing about.
        let mnemonic = match self.resolve_funded_identity().await {
            Ok(m) => m,
            Err(reason) => anyhow::bail!("{reason}"),
        };

        {
            let mut sup = self.supervisor.lock().unwrap();
            let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            // The mnemonic goes in the ENVIRONMENT, never in argv — argv is
            // readable by any process via tasklist/WMI.
            sup.start_integration_with_env(
                "fryvpn",
                &binary,
                &arg_refs,
                &[("NODE_MNEMONIC", mnemonic.as_str())],
            )
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
            // BUG 6: say why instead of a bare Stopped (see
            // `process_not_running_reason`).
            let stderr_path = self.log_dir.join("fryvpn").join("fryvpn_stderr.log");
            let stderr_content = tokio::fs::read_to_string(&stderr_path)
                .await
                .unwrap_or_default();
            let tail = super::stderr_tail(&stderr_content, 3);
            let tail = if tail == "no error output" { String::new() } else { tail };
            return HealthStatus::Unhealthy(process_not_running_reason(&tail));
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
            config: Arc::new(crate::config::store::ConfigStore::new(std::path::PathBuf::from("/tmp"), None)),
            // BUG 6: field added so fryvpn can fetch the device's provisioned
            // Algorand identity. Fixture value only — these two tests assert on
            // id()/display_name() and never touch the client. No assertion changed.
            api_client: Arc::new(crate::api::client::ApiClient::new(
                "http://127.0.0.1:1".to_string(),
                String::new(),
            )),
            supervisor: Arc::new(Mutex::new(crate::supervisor::Supervisor::new(
                std::path::PathBuf::from("/tmp"),
            ))),
            log_dir: std::path::PathBuf::from("/tmp"),
        };
        assert_eq!(integration.id(), "fryvpn");
    }

    #[test]
    fn test_fryvpn_display_name() {
        let integration = FryVpnIntegration {
            config: Arc::new(crate::config::store::ConfigStore::new(std::path::PathBuf::from("/tmp"), None)),
            // BUG 6: field added so fryvpn can fetch the device's provisioned
            // Algorand identity. Fixture value only — these two tests assert on
            // id()/display_name() and never touch the client. No assertion changed.
            api_client: Arc::new(crate::api::client::ApiClient::new(
                "http://127.0.0.1:1".to_string(),
                String::new(),
            )),
            supervisor: Arc::new(Mutex::new(crate::supervisor::Supervisor::new(
                std::path::PathBuf::from("/tmp"),
            ))),
            log_dir: std::path::PathBuf::from("/tmp"),
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

    /// BUG 6: "Fry dVPN card 'STARTING' forever, dashboard 'Unhealthy', no
    /// reason" — a not-running process must always carry a reason.
    #[test]
    fn a_dead_process_with_no_log_output_still_gets_a_concrete_reason() {
        let reason = process_not_running_reason("");
        assert_eq!(reason, "frynode process is not running");
    }

    #[test]
    fn a_dead_process_with_log_output_includes_it_in_the_reason() {
        let reason = process_not_running_reason("failed to load config: REGION is required");
        assert!(reason.contains("frynode process is not running"), "{reason}");
        assert!(reason.contains("REGION is required"), "{reason}");
    }

    #[test]
    fn the_firewall_rule_name_is_dedicated_to_frynode() {
        assert_eq!(FRYNODE_RULE_NAME, "FEM-FryNode");
        assert_ne!(
            FRYNODE_RULE_NAME,
            super::super::firewall::OLOSTEP_RULE_NAME,
            "must not collide with Olostep's rule"
        );
    }
}

/// BUG 6 (minerman): fryDVPN on-chain registration fails — wallet Z2HCY…GTMKI
/// has 0 ALGO, so `RegisterNode` is rejected with an overspend error and the
/// card shows "dVPN not registered on-chain" with no way to act on it.
///
/// Root cause: FEM passed frynode no node identity at all, so frynode
/// generated a FRESH Algorand account with a zero balance and immediately
/// tried to pay a transaction fee from it. FEM already holds the device's
/// provisioned credentials (`CredentialInfo.algo_address` / `.algo_mnemonic`)
/// — Diiisco consumes exactly these — but `FryVpnIntegration` had no
/// `api_client` to fetch them with.
#[cfg(test)]
mod bug6_balance_tests {
    use super::*;

    #[test]
    fn a_zero_balance_wallet_is_refused_before_any_transaction_is_attempted() {
        let err = registration_affordability(0, 100_000, "Z2HCYEXAMPLEADDRESS")
            .expect_err("0 ALGO must not proceed to an on-chain call");
        assert!(err.contains("Z2HCYEXAMPLEADDRESS"), "the user must be told WHICH wallet: {err}");
        assert!(err.contains("ALGO"), "the user must be told what to add: {err}");
        assert!(
            !err.to_lowercase().contains("overspend"),
            "must not surface the raw blockchain error: {err}"
        );
    }

    #[test]
    fn a_funded_wallet_proceeds() {
        assert_eq!(registration_affordability(500_000, 100_000, "ADDR"), Ok(()));
        // Exactly at the threshold is fine.
        assert_eq!(registration_affordability(100_000, 100_000, "ADDR"), Ok(()));
    }

    /// The shortfall must be quantified, not just "insufficient" — the user
    /// needs to know how much to send.
    #[test]
    fn the_message_quantifies_the_shortfall_in_whole_algo() {
        let err = registration_affordability(25_000, 100_000, "ADDR").unwrap_err();
        assert!(err.contains("0.075"), "shortfall in ALGO must be explicit: {err}");
    }

    /// Balance parsing must come from the real algod account shape.
    #[test]
    fn the_spendable_balance_excludes_the_locked_minimum() {
        // algod reports total `amount` and the locked `min-balance`.
        assert_eq!(spendable_microalgos(300_000, 200_000), 100_000);
        // A wallet below its own minimum has nothing spendable, never negative.
        assert_eq!(spendable_microalgos(100_000, 200_000), 0);
    }

    #[test]
    fn a_real_algod_account_response_parses() {
        let body = r#"{"address":"ABC","amount":1234567,"min-balance":100000,"round":12345}"#;
        assert_eq!(parse_account_balance(body), Some((1_234_567, 100_000)));
    }

    #[test]
    fn an_unparseable_algod_response_is_not_treated_as_zero() {
        assert_eq!(parse_account_balance("<html>502 Bad Gateway</html>"), None);
        assert_eq!(parse_account_balance(""), None);
    }
}
