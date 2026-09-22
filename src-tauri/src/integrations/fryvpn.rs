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

    /// The algod endpoint FEM reads the device wallet from, and the one it
    /// hands frynode. Defaults to the shipped mainnet endpoint, so nothing
    /// changes for existing users; the override exists so the registration
    /// path can be exercised against AlgoKit LocalNet, and so a throttled or
    /// blocked public endpoint can be redirected without a rebuild. Same shape
    /// as `region()`/`capacity_mbps()`/`binary_path()`.
    fn algod_server() -> String {
        std::env::var("FRYNODE_ALGOD_SERVER")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| ALGOD_SERVER.to_string())
    }

    fn algod_port() -> String {
        std::env::var("FRYNODE_ALGOD_PORT")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "443".to_string())
    }

    /// Empty by default — algonode is tokenless. LocalNet is not.
    fn algod_token() -> String {
        std::env::var("FRYNODE_ALGOD_TOKEN").unwrap_or_default()
    }

    fn registry_app_id() -> String {
        std::env::var("FRYNODE_REGISTRY_APP_ID")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "3636586918".to_string())
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

/// The payment frynode makes to fund this node's on-chain registry box —
/// `defaultMBR` in the node's `registry` package. It is a real transfer out of
/// the device wallet, not a fee, and leaving it out of the requirement is what
/// let an underfunded wallet pass the pre-check and then overspend on chain.
pub(crate) const REGISTRY_BOX_MBR_MICROALGOS: u64 = 200_000;

/// Algorand's network minimum fee.
pub(crate) const ALGORAND_MIN_FEE_MICROALGOS: u64 = 1_000;

/// The register group is exactly two transactions — the box MBR payment and
/// the `register_node` app call — and neither overrides `FlatFee`, so both pay
/// the network minimum.
pub(crate) const REGISTRATION_GROUP_TXNS: u64 = 2;

/// Documented headroom over the measured cost, so a min-fee change or a
/// slightly larger box does not strand a wallet that funded exactly what the
/// card asked for.
pub(crate) const REGISTRATION_MARGIN_MICROALGOS: u64 = 10_000;

/// Minimum SPENDABLE balance (microAlgos) fryDVPN needs to register on-chain.
///
/// Derived from the group frynode actually submits rather than guessed: the
/// previous value (100_000) counted the fees and forgot the box payment, so it
/// was less than half the true cost. This is the SPENDABLE requirement — an
/// account must additionally retain its own base minimum balance, which is why
/// the card quotes `total_needed` (see `registration_funding_message`) and not
/// this number.
pub(crate) const REGISTRATION_MIN_MICROALGOS: u64 = REGISTRY_BOX_MBR_MICROALGOS
    + REGISTRATION_GROUP_TXNS * ALGORAND_MIN_FEE_MICROALGOS
    + REGISTRATION_MARGIN_MICROALGOS;

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

/// B7/B8: the funding instruction `start()` parks when the device wallet
/// cannot afford registration yet.
///
/// Process-global rather than a field on `FryVpnIntegration`: the struct is
/// built once in `main.rs` and in two existing tests, and a new required field
/// would have forced an edit to all three — including test code this run is
/// not permitted to touch. There is exactly one fryDVPN integration per
/// process, so a module-level slot is the same state with a smaller diff.
static PARKED_FUNDING_REASON: Mutex<Option<String>> = Mutex::new(None);

/// The prefix every fryDVPN funding message starts with.
///
/// Load-bearing, not decoration: `integrations::awaits_user_action` matches it,
/// which is what makes the supervisor treat "this wallet needs money" as a
/// setup state (RecoveryAction::None) instead of a fault it restarts frynode
/// over every 30 s, and what makes the card render amber "Setup required"
/// instead of a red UNHEALTHY badge.
pub(crate) const FUNDING_MARKER: &str = "Awaiting fryDVPN funding";

/// PURE: the funding instruction the card shows while the device wallet cannot
/// afford registration.
///
/// Reports TOTAL NEEDED against the on-chain `amount` — the number the owner
/// sees in their wallet app. The message this replaces was built from SPENDABLE
/// on both sides, so a user who sent exactly the 0.1 ALGO it asked for was then
/// told he held "0.000 ALGO" (mainnet: amount 100_014, min-balance 100_000,
/// spendable 14). Total needed is the spendable requirement plus the account's
/// own locked minimum, because that minimum has to stay behind after the group
/// settles.
///
/// The gate itself is still `registration_affordability`, so that function and
/// its tests remain the authority on WHETHER to proceed; only the DISPLAY
/// changed.
pub(crate) fn registration_funding_message(
    amount: u64,
    min_balance: u64,
    required_spendable: u64,
    address: &str,
) -> Result<(), String> {
    if registration_affordability(
        spendable_microalgos(amount, min_balance),
        required_spendable,
        address,
    )
    .is_ok()
    {
        return Ok(());
    }
    let total_needed = required_spendable.saturating_add(min_balance);
    let short = total_needed.saturating_sub(amount);
    Err(format!(
        "{FUNDING_MARKER} — send {:.3} ALGO to {} (this device's wallet needs {:.3} ALGO total to register on-chain and holds {:.3} ALGO). fryDVPN registers automatically on the next check.",
        short as f64 / 1_000_000.0,
        address,
        total_needed as f64 / 1_000_000.0,
        amount as f64 / 1_000_000.0
    ))
}

/// What the card says when algod could not be read at all.
///
/// Deliberately quotes NO figure: an unreachable endpoint or an HTML error page
/// must never be rendered as "0.000 ALGO", which would tell a funded owner to
/// send money they have already sent.
pub(crate) fn balance_unreadable_message(detail: &str) -> String {
    format!("{FUNDING_MARKER} — could not read this device's wallet balance ({detail}); retrying on the next check.")
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

/// What to do about frynode's node identity (BUG 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IdentityPlan {
    /// We hold the device's key: pass it, after a balance pre-check.
    UseIdentity { address: String, mnemonic: String },
    /// We do not hold a usable key. Start frynode exactly as before rather
    /// than removing a node that would otherwise run.
    StartWithoutIdentity,
}

/// PURE: decide from what hardwareapi actually returned.
pub(crate) fn identity_plan(address: Option<&str>, mnemonic: Option<&str>) -> IdentityPlan {
    match (
        address.map(str::trim).filter(|a| !a.is_empty()),
        mnemonic.map(str::trim).filter(|m| !m.is_empty()),
    ) {
        (Some(a), Some(m)) => IdentityPlan::UseIdentity {
            address: a.to_string(),
            mnemonic: m.to_string(),
        },
        _ => IdentityPlan::StartWithoutIdentity,
    }
}

/// What a read of the device wallet concluded (B7/B8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FundingState {
    /// Measured, and the wallet can afford the registration group.
    Affordable,
    /// Measured, and it cannot yet. The string is the card's funding
    /// instruction, already carrying `FUNDING_MARKER`.
    Underfunded(String),
    /// Not measurable right now — algod unreachable, or an error page where
    /// JSON was expected. Never a reason to refuse to run a node that would
    /// otherwise serve traffic: frynode's own preflight is what stops an
    /// underfunded submission reaching the chain.
    Unmeasurable(String),
}

impl FryVpnIntegration {
    /// One algod account read.
    ///
    /// `Err` carries a short reason for the card and never a balance — an HTML
    /// error page or a rate-limit body read as "0" would tell an owner who has
    /// already funded the wallet to send more.
    async fn read_wallet_balance(address: &str) -> Result<(u64, u64), String> {
        let url = format!("{}/v2/accounts/{}", Self::algod_server(), address);
        let resp = reqwest::Client::new()
            .get(&url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| format!("algod unreachable: {e}"))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| format!("algod response unreadable: {e}"))?;
        parse_account_balance(&body).ok_or_else(|| format!("algod returned HTTP {status}"))
    }

    /// The device's provisioned Algorand identity together with what its wallet
    /// can currently afford (BUG 6, B7, B8).
    ///
    /// `None` when we hold no usable key: frynode then starts exactly as it did
    /// before BUG 6, because refusing to run a node over a wallet we cannot
    /// even identify would remove a node that works.
    ///
    /// Split out from `resolve_funded_identity` because the health loop needs
    /// the funding state on its own, to clear a parked funding card the moment
    /// the wallet is funded — without that, the figure on the card froze at
    /// whatever the one failed toggle measured.
    async fn device_identity_and_funding(&self) -> Option<(String, FundingState)> {
        let miner_key = self.config.get().miner_key?;

        let creds = match crate::api::credentials::lookup(&self.api_client, &miner_key).await {
            Ok(c) => c,
            Err(e) => {
                // A lookup failure must not remove a node that would run.
                warn!(error = %e, "Could not fetch device wallet - starting fryDVPN without it");
                return None;
            }
        };

        match identity_plan(
            creds.algo_address.as_deref(),
            creds.algo_mnemonic.as_deref(),
        ) {
            IdentityPlan::StartWithoutIdentity => {
                info!("No device wallet key available - starting fryDVPN without a node identity");
                None
            }
            IdentityPlan::UseIdentity { address, mnemonic } => {
                // We hold this key, so the balance we measure IS the account
                // frynode will spend from - only now is refusing justified.
                let state = match Self::read_wallet_balance(&address).await {
                    Ok((amount, min_balance)) => match registration_funding_message(
                        amount,
                        min_balance,
                        REGISTRATION_MIN_MICROALGOS,
                        &address,
                    ) {
                        Ok(()) => FundingState::Affordable,
                        Err(msg) => FundingState::Underfunded(msg),
                    },
                    Err(detail) => {
                        warn!(
                            detail = %detail,
                            "Could not read the fryDVPN wallet balance - starting without a pre-check"
                        );
                        FundingState::Unmeasurable(detail)
                    }
                };
                Some((mnemonic, state))
            }
        }
    }

    /// Fetch the device's provisioned Algorand identity and confirm it can
    /// actually afford the on-chain registration (BUG 6).
    ///
    /// Returns the mnemonic to hand frynode. Any failure is a user-facing
    /// sentence, never a raw chain error.
    async fn resolve_funded_identity(&self) -> Result<Option<String>, String> {
        match self.device_identity_and_funding().await {
            None => Ok(None),
            Some((_, FundingState::Underfunded(msg))) => Err(msg),
            Some((mnemonic, _)) => Ok(Some(mnemonic)),
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
            Self::registry_app_id(),
            "-fvpn-asa-id".to_string(),
            "2485198745".to_string(),
            "-algod-server".to_string(),
            Self::algod_server(),
            "-algod-port".to_string(),
            Self::algod_port(),
            "-algod-token".to_string(),
            Self::algod_token(),
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
            Ok(m) => {
                *PARKED_FUNDING_REASON.lock().unwrap() = None;
                m
            }
            // B7/B8: an unaffordable wallet is a SETUP state, not a start
            // failure. Bailing here returned from `toggle_integration` BEFORE
            // `set_enabled`, so the toggle flipped itself back off, the health
            // loop short-circuited to Stopped while disabled, and the balance
            // was never read again — the funding figure froze at whatever that
            // one failed toggle measured and the owner had to keep toggling.
            // Park the instruction and succeed WITHOUT spawning frynode:
            // nothing is submitted on chain, the integration stays enabled, and
            // `health_check` re-reads the wallet every tick until it is funded.
            Err(reason) => {
                warn!(
                    reason = %reason,
                    "fryDVPN registration deferred until the device wallet is funded"
                );
                *PARKED_FUNDING_REASON.lock().unwrap() = Some(reason);
                return Ok(());
            }
        };

        {
            let mut sup = self.supervisor.lock().unwrap();
            let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            // The mnemonic goes in the ENVIRONMENT, never in argv — argv is
            // readable by any process via tasklist/WMI.
            let env: Vec<(&str, &str)> = match mnemonic.as_deref() {
                Some(m) => vec![("NODE_MNEMONIC", m)],
                None => Vec::new(),
            };
            sup.start_integration_with_env("fryvpn", &binary, &arg_refs, &env)
                .map_err(|e| anyhow::anyhow!("Failed to spawn frynode: {}", e))?;
        }

        info!("Fry dVPN started with CLI flags");
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        // A parked funding instruction describes a start that never happened;
        // drop it so re-enabling measures the wallet again rather than showing
        // a stale figure.
        *PARKED_FUNDING_REASON.lock().unwrap() = None;
        {
            let mut sup = self.supervisor.lock().unwrap();
            sup.stop_integration("fryvpn")
                .map_err(|e| anyhow::anyhow!("Failed to stop frynode: {}", e))?;
        }
        info!("Fry dVPN stopped");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        // B7/B8: while a funding instruction is parked, frynode was never
        // spawned, so re-read the wallet instead of reporting a dead process.
        // This is what makes "registers automatically on the next check" true:
        // the moment the wallet is funded the park clears, the not-running
        // branch below arms the supervisor's existing restart, and `start()`
        // re-runs against a wallet that can pay — with no user toggling.
        let parked = PARKED_FUNDING_REASON.lock().unwrap().is_some();
        if parked {
            match self.device_identity_and_funding().await {
                Some((_, FundingState::Underfunded(msg))) => {
                    *PARKED_FUNDING_REASON.lock().unwrap() = Some(msg.clone());
                    return HealthStatus::Unhealthy(msg);
                }
                // Keep the park rather than churning frynode while algod is
                // unreadable, and never quote a balance we could not measure.
                Some((_, FundingState::Unmeasurable(detail))) => {
                    return HealthStatus::Unhealthy(balance_unreadable_message(&detail));
                }
                // Affordable now, or we no longer hold a key to measure.
                _ => *PARKED_FUNDING_REASON.lock().unwrap() = None,
            }
        }

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
            let tail = if tail == "no error output" {
                String::new()
            } else {
                tail
            };
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

    /// FIX 5: tests that mutate process-global environment variables must not
    /// run concurrently. `cargo test` runs tests as threads in ONE process, so
    /// a `set_var`/`remove_var` in one test is immediately visible to every
    /// other. Reproduced before this fix: 1 failure in 10 consecutive parallel
    /// full-suite runs (`region_honours_the_env_override`).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Holds `ENV_LOCK` and restores the variable's ORIGINAL value on drop.
    ///
    /// The restore lives in `Drop`, not at the end of the test body, so a
    /// panic mid-test cannot leak a mutated variable into the next test.
    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn acquire(key: &'static str) -> Self {
            // A panicking test poisons the mutex; recover the guard rather
            // than cascading one real failure into unrelated ones.
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            Self {
                key,
                prev: std::env::var(key).ok(),
                _lock: lock,
            }
        }

        fn set(&self, value: &str) {
            std::env::set_var(self.key, value);
        }

        fn unset(&self) {
            std::env::remove_var(self.key);
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn test_fryvpn_id() {
        let integration = FryVpnIntegration {
            config: Arc::new(crate::config::store::ConfigStore::new(
                std::path::PathBuf::from("/tmp"),
                None,
            )),
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
            config: Arc::new(crate::config::store::ConfigStore::new(
                std::path::PathBuf::from("/tmp"),
                None,
            )),
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
        assert!(
            resolved.contains("resources"),
            "must be the full resource path: {resolved}"
        );
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
        let env = EnvGuard::acquire("FRYNODE_REGION");
        env.unset();
        assert_eq!(FryVpnIntegration::region(), "us");
    }

    #[test]
    fn region_honours_the_env_override() {
        let env = EnvGuard::acquire("FRYNODE_REGION");
        env.set("eu-west");
        assert_eq!(FryVpnIntegration::region(), "eu-west");
        env.set("   ");
        assert_eq!(
            FryVpnIntegration::region(),
            "us",
            "blank override must fall back"
        );
    }

    /// FIX 5: the guard must restore the ORIGINAL value even when the test
    /// holding it panics. A cleanup line at the end of a test body cannot do
    /// this, which is why the restore lives in `Drop`.
    ///
    /// NOTE: `EnvGuard` is NOT re-entrant — it holds a plain `Mutex`, so
    /// acquiring it twice on one thread deadlocks. This test therefore seeds
    /// the variable directly and takes the guard exactly once, inside the
    /// panicking closure.
    #[test]
    fn the_env_guard_restores_the_original_value_even_after_a_panic() {
        const KEY: &str = "FEM_ENVGUARD_PROBE";
        std::env::set_var(KEY, "original");

        let panicked = std::panic::catch_unwind(|| {
            let inner = EnvGuard::acquire(KEY);
            inner.set("clobbered");
            panic!("simulated test failure while holding the guard");
        });
        assert!(panicked.is_err(), "the inner closure must have panicked");

        assert_eq!(
            std::env::var(KEY).ok().as_deref(),
            Some("original"),
            "Drop must restore the pre-guard value even when the test panicked"
        );

        std::env::remove_var(KEY);
        let absent = std::panic::catch_unwind(|| {
            let inner = EnvGuard::acquire(KEY);
            inner.set("temporary");
            panic!("panic with no prior value");
        });
        assert!(absent.is_err());
        assert!(
            std::env::var(KEY).is_err(),
            "a variable that did not exist before the guard must be unset again"
        );
    }

    #[test]
    fn capacity_is_always_positive() {
        // frynode rejects 0 outright: "CAPACITY_MBPS is required (must be > 0)".
        let env = EnvGuard::acquire("FRYNODE_CAPACITY_MBPS");
        env.unset();
        assert!(FryVpnIntegration::capacity_mbps() > 0);

        env.set("250");
        assert_eq!(FryVpnIntegration::capacity_mbps(), 250);

        for bad in ["0", "-5", "abc", ""] {
            env.set(bad);
            assert!(
                FryVpnIntegration::capacity_mbps() > 0,
                "override {bad:?} must not produce a zero capacity"
            );
        }
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
        assert!(
            reason.contains("frynode process is not running"),
            "{reason}"
        );
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

    /// B8: the registration path was pinned to mainnet in two independent
    /// literals, so it could not be exercised against LocalNet at all and a
    /// throttled endpoint could not be redirected without a rebuild. These
    /// live in `mod tests` rather than a module of their own because they
    /// mutate process-global environment and must share `ENV_LOCK` with the
    /// other env tests.
    #[test]
    fn the_algod_endpoint_defaults_to_mainnet_and_honours_the_override() {
        let g = EnvGuard::acquire("FRYNODE_ALGOD_SERVER");
        g.unset();
        assert_eq!(
            FryVpnIntegration::algod_server(),
            "https://mainnet-api.algonode.cloud"
        );
        g.set("http://127.0.0.1:4001");
        assert_eq!(FryVpnIntegration::algod_server(), "http://127.0.0.1:4001");
        g.set("   ");
        assert_eq!(
            FryVpnIntegration::algod_server(),
            "https://mainnet-api.algonode.cloud",
            "a blank override must fall back, never produce an empty URL"
        );
    }

    #[test]
    fn the_registry_app_id_defaults_to_mainnet_and_honours_the_override() {
        let g = EnvGuard::acquire("FRYNODE_REGISTRY_APP_ID");
        g.unset();
        assert_eq!(FryVpnIntegration::registry_app_id(), "3636586918");
        g.set("1011");
        assert_eq!(FryVpnIntegration::registry_app_id(), "1011");
    }

    #[test]
    fn the_algod_port_defaults_to_https_and_honours_the_override() {
        let g = EnvGuard::acquire("FRYNODE_ALGOD_PORT");
        g.unset();
        assert_eq!(FryVpnIntegration::algod_port(), "443");
        g.set("4001");
        assert_eq!(FryVpnIntegration::algod_port(), "4001");
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
        assert!(
            err.contains("Z2HCYEXAMPLEADDRESS"),
            "the user must be told WHICH wallet: {err}"
        );
        assert!(
            err.contains("ALGO"),
            "the user must be told what to add: {err}"
        );
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
        assert!(
            err.contains("0.075"),
            "shortfall in ALGO must be explicit: {err}"
        );
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

/// BUG 6 follow-up, found by the LIVE CANARY and not by the unit tests.
///
/// The first cut refused to start frynode whenever the device's mnemonic was
/// unavailable. On a real device (algo_address present, algo_mnemonic absent --
/// hardwareapi only returns the mnemonic when its encrypted blob decrypts) that
/// is a REGRESSION: before the change frynode still ran and served traffic, it
/// just failed the on-chain registration. Refusing to start removes a working
/// node in order to fix a registration problem.
#[cfg(test)]
mod bug6_identity_fallback_tests {
    use super::*;

    #[test]
    fn a_missing_mnemonic_falls_back_instead_of_blocking_the_node() {
        assert_eq!(
            identity_plan(Some("ZWWFC7ADDR"), None),
            IdentityPlan::StartWithoutIdentity
        );
        assert_eq!(
            identity_plan(None, None),
            IdentityPlan::StartWithoutIdentity
        );
    }

    #[test]
    fn a_usable_mnemonic_is_passed_through() {
        let m = "word ".repeat(25);
        let m = m.trim();
        assert_eq!(
            identity_plan(Some("ADDR"), Some(m)),
            IdentityPlan::UseIdentity {
                address: "ADDR".to_string(),
                mnemonic: m.to_string(),
            }
        );
    }

    #[test]
    fn a_blank_mnemonic_is_treated_as_absent() {
        assert_eq!(
            identity_plan(Some("ADDR"), Some("   ")),
            IdentityPlan::StartWithoutIdentity
        );
    }

    /// The whole point: an underfunded wallet is only ever REFUSED when we
    /// actually hold that wallet's key, because only then is the balance we
    /// measured the account frynode will really spend from.
    #[test]
    fn refusal_requires_actually_holding_the_identity() {
        assert!(
            !matches!(
                identity_plan(Some("ADDR"), None),
                IdentityPlan::UseIdentity { .. }
            ),
            "without the key we cannot know which account frynode will use, so we must not \
             block on a balance we did not measure"
        );
    }
}

/// B7/B8: the registration requirement must be the cost of the group frynode
/// ACTUALLY submits, not a round number.
///
/// frynode's `RegisterNode` (node/registry/registry.go) composes a two-txn
/// group — a payment of `defaultMBR` = 200_000 µALGO to the registry app
/// address, which funds this node's on-chain box, plus the `register_node`
/// app call — and neither txn sets `FlatFee`, so both pay the network minimum
/// fee of 1_000. The shipped requirement of 100_000 counted the fees and
/// forgot the box payment entirely, so a wallet that PASSED this pre-check
/// still had its registration rejected on chain with an `overspend` the owner
/// could do nothing about.
#[cfg(test)]
mod b7_registration_cost_tests {
    use super::*;

    #[test]
    fn the_requirement_covers_the_whole_registration_group() {
        assert!(
            REGISTRATION_MIN_MICROALGOS >= 202_000,
            "the pre-check must cover the box MBR AND both fees, not just the fees: \
             {REGISTRATION_MIN_MICROALGOS}"
        );
    }

    #[test]
    fn the_requirement_is_the_measured_cost_plus_the_documented_margin() {
        // 200_000 box MBR + 2 x 1_000 min fee = 202_000 measured, plus a
        // 10_000 µALGO documented margin covering a min-fee change and
        // box-size variation.
        assert_eq!(REGISTRATION_MIN_MICROALGOS, 212_000);
    }

    /// §6 B8 Done-when: boundary tests at 0, requirement-1, requirement and
    /// requirement+1. These run against whatever the constant is, so they keep
    /// pinning the gate if the derivation ever changes.
    #[test]
    fn the_gate_is_exact_at_its_boundaries() {
        let req = REGISTRATION_MIN_MICROALGOS;
        assert!(registration_affordability(0, req, "ADDR").is_err());
        assert!(registration_affordability(req - 1, req, "ADDR").is_err());
        assert!(registration_affordability(req, req, "ADDR").is_ok());
        assert!(registration_affordability(req + 1, req, "ADDR").is_ok());
    }
}

/// B8: "I sent 0.1 Algo to the address in the error message, but it still says
/// 0.000 ALGO after a few hours."
///
/// The card was built from SPENDABLE on both sides — required and held — so a
/// wallet holding exactly the 0.1 ALGO the card asked for displayed as 0.000:
/// on mainnet that account reads amount=100_014 / min-balance=100_000, and
/// 100_014 - 100_000 = 14 µALGO formats as "0.000". These pin the display to
/// TOTAL NEEDED vs the on-chain `amount`, which is the number the owner sees in
/// their wallet.
#[cfg(test)]
mod b8_funding_display_tests {
    use super::*;

    const MIN_BAL: u64 = 100_000;

    #[test]
    fn a_wallet_holding_0_1_algo_is_never_reported_as_holding_nothing() {
        let msg =
            registration_funding_message(100_014, MIN_BAL, REGISTRATION_MIN_MICROALGOS, "ADDR")
                .expect_err("14 µALGO spendable cannot afford registration");
        assert!(
            !msg.contains("0.000"),
            "a wallet holding 0.100 ALGO on chain must never be displayed as 0.000: {msg}"
        );
        assert!(
            msg.contains("0.100"),
            "the card must quote the on-chain amount the owner can see: {msg}"
        );
        assert!(
            msg.contains("0.312"),
            "the card must quote TOTAL needed, not the spendable requirement: {msg}"
        );
        assert!(
            msg.contains("0.212"),
            "the card must name exactly how much more to send: {msg}"
        );
        assert!(msg.contains("ADDR"), "and where to send it: {msg}");
    }

    #[test]
    fn an_empty_wallet_is_asked_for_the_whole_total() {
        let msg = registration_funding_message(0, MIN_BAL, REGISTRATION_MIN_MICROALGOS, "ADDR")
            .unwrap_err();
        assert!(
            msg.contains("send 0.312 ALGO"),
            "an empty wallet must be told the account minimum too: {msg}"
        );
    }

    /// §6 B8 Done-when: boundaries at 0, requirement-1, requirement,
    /// requirement+1 — expressed in on-chain `amount`, which is what the user
    /// actually sends.
    #[test]
    fn the_displayed_total_is_exact_at_its_boundaries() {
        let total = REGISTRATION_MIN_MICROALGOS + MIN_BAL;
        assert_eq!(total, 312_000);
        assert!(registration_funding_message(0, MIN_BAL, REGISTRATION_MIN_MICROALGOS, "A").is_err());
        assert!(
            registration_funding_message(total - 1, MIN_BAL, REGISTRATION_MIN_MICROALGOS, "A")
                .is_err()
        );
        assert!(
            registration_funding_message(total, MIN_BAL, REGISTRATION_MIN_MICROALGOS, "A").is_ok()
        );
        assert!(
            registration_funding_message(total + 1, MIN_BAL, REGISTRATION_MIN_MICROALGOS, "A")
                .is_ok()
        );
    }

    /// The message the users quoted contained two runs of ten spaces, from the
    /// Rust source being line-wrapped inside a string literal.
    #[test]
    fn the_funding_message_has_no_run_on_whitespace() {
        let msg = registration_funding_message(0, MIN_BAL, REGISTRATION_MIN_MICROALGOS, "ADDR")
            .unwrap_err();
        assert!(!msg.contains("  "), "double space rendered verbatim: {msg}");
    }

    /// An unreachable or rate-limited endpoint must never be rendered as a
    /// balance — telling an owner who has already funded the wallet that it
    /// holds 0.000 is exactly the bug this run is fixing.
    #[test]
    fn an_unreadable_balance_quotes_no_figure_at_all() {
        let msg = balance_unreadable_message("algod returned HTTP 403 Forbidden");
        assert!(!msg.contains("0.000"), "{msg}");
        assert!(!msg.contains("ALGO)"), "{msg}");
        assert!(msg.contains("could not read"), "{msg}");
        assert!(crate::integrations::awaits_user_action(&msg));
    }
}

/// B7/B8: an unfunded device wallet is a SETUP state, not a red failure and
/// not something to restart frynode over.
///
/// Before this, the funding message matched neither awaiting-marker, so
/// `recovery_action` returned `Restart` and the card rendered as a red
/// UNHEALTHY badge instead of the amber "Setup required" the frontend already
/// has for exactly this case.
#[cfg(test)]
mod b7_funding_state_tests {
    use super::*;
    use crate::supervisor::health::{recovery_action, RecoveryAction};

    #[test]
    fn an_unfunded_wallet_is_a_setup_state_not_a_red_failure() {
        let msg = registration_funding_message(0, 100_000, REGISTRATION_MIN_MICROALGOS, "ADDR")
            .unwrap_err();
        assert!(
            msg.starts_with(FUNDING_MARKER),
            "the marker must be a PREFIX — the frontend matches it with startsWith: {msg}"
        );
        assert!(
            crate::integrations::awaits_user_action(&msg),
            "must be recognised as awaiting the user: {msg}"
        );
    }

    #[test]
    fn a_funding_reason_is_never_restarted() {
        let msg = registration_funding_message(0, 100_000, REGISTRATION_MIN_MICROALGOS, "ADDR")
            .unwrap_err();
        assert_eq!(
            recovery_action(&HealthStatus::Unhealthy(msg), true, 0, 6),
            RecoveryAction::None,
            "restarting frynode cannot make a wallet richer, and doing it every 30 s is the \
             restart storm users reported"
        );
    }

    /// Guard on the marker set itself: widening it must not swallow a real
    /// fault that the supervisor SHOULD recover from.
    #[test]
    fn the_new_marker_does_not_swallow_real_frynode_faults() {
        assert!(!crate::integrations::awaits_user_action(
            &process_not_running_reason("failed to load config: REGION is required")
        ));
        assert!(!crate::integrations::awaits_user_action(
            "frynode process is not running"
        ));
    }
}
