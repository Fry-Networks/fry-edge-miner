use super::{HealthStatus, Integration, PocGateData};
use crate::api::client::ApiClient;
use crate::api::types::CredentialInfo;
use crate::config::store::ConfigStore;
use crate::supervisor::platform::BoundedOutput;
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{info, warn};

#[path = "diiisco_node_key.rs"]
mod diiisco_node_key;
#[path = "diiisco_rebuild.rs"]
mod diiisco_rebuild;

const HEALTH_URL: &str = "http://localhost:8181/health";

/// What the node-health HTTP request came back with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum HttpProbe {
    Status(reqwest::StatusCode),
    Refused,
    Timeout,
}

/// The `docker` argv that lists the running `diiisco-node` container.
#[allow(dead_code)]
fn node_container_ps_args() -> Vec<&'static str> {
    Vec::new()
}

/// Before the HTTP probe: is the node container there at all? `None` = the
/// check itself failed.
#[allow(dead_code)]
fn health_gate(_node_running: Option<bool>) -> Result<(), HealthStatus> {
    Ok(())
}

#[allow(dead_code)]
fn http_health(probe: HttpProbe) -> HealthStatus {
    match probe {
        HttpProbe::Status(s) if s.is_success() => HealthStatus::Healthy,
        HttpProbe::Status(s) => HealthStatus::Unhealthy(format!("HTTP {s}")),
        HttpProbe::Refused => HealthStatus::Stopped,
        HttpProbe::Timeout => HealthStatus::Unhealthy("Timeout".to_string()),
    }
}

#[allow(dead_code)]
fn classify_health(node_running: Option<bool>, http: HttpProbe) -> HealthStatus {
    match health_gate(node_running) {
        Err(status) => status,
        Ok(()) => http_health(http),
    }
}

/// B6: what the user is told when hardwareapi has no Algorand wallet for this
/// device yet. A missing wallet is a provisioning state, not a fault — the old
/// "Device has no Algorand wallet — re-register or contact support" read as a
/// crash and gave no first step.
pub const WALLET_NOT_PROVISIONED: &str =
    "Device wallet not provisioned yet — open Settings and re-register this device, or contact support.";

/// How long a proven "this device has no wallet" answer is trusted before FEM
/// asks hardwareapi again. Long enough that a device waiting on provisioning
/// stops hammering the API on every supervisor retry, short enough that a
/// wallet provisioned in the meantime is picked up without a restart.
pub const WALLET_CACHE_TTL: Duration = Duration::from_secs(15 * 60);

/// When the last lookup proved the device has no Algorand wallet, if it is
/// still within `WALLET_CACHE_TTL`.
///
/// `check_requirements()` is synchronous — the PoC reporter calls it on every
/// tick — so it can never do the credentials lookup itself. This cache is how
/// the async discovery in `install()`/`start()` reaches it. Routing the
/// condition through `unavailable_reason` instead of a start error is the
/// whole point: the card goes inert with a sentence the user can act on, the
/// toggle refuses up front, and the health loop stops restarting something
/// that provably cannot start yet.
static NO_WALLET_SINCE: Mutex<Option<Instant>> = Mutex::new(None);

/// Classify a credentials lookup into the wallet address or the user-facing
/// reason the integration cannot run yet. A blank address counts as missing:
/// hardwareapi returns `""` for a device whose wallet row exists but has not
/// been funded with an address.
pub fn classify_wallet(algo_address: Option<&str>) -> Result<String, &'static str> {
    match algo_address {
        Some(addr) if !addr.trim().is_empty() => Ok(addr.trim().to_string()),
        _ => Err(WALLET_NOT_PROVISIONED),
    }
}

/// Whether a miss recorded at `since` is still authoritative at `now`.
/// Pure so the TTL can be tested without waiting a quarter of an hour.
pub fn cache_is_live(since: Option<Instant>, now: Instant, ttl: Duration) -> bool {
    match since {
        Some(t) => now.saturating_duration_since(t) < ttl,
        None => false,
    }
}

/// Record the observed wallet state. Logs only on a transition, so a device
/// waiting on provisioning does not write a line every health tick.
fn record_wallet_state(provisioned: bool) {
    let mut slot = match NO_WALLET_SINCE.lock() {
        Ok(s) => s,
        Err(poisoned) => poisoned.into_inner(),
    };
    let was_missing = cache_is_live(*slot, Instant::now(), WALLET_CACHE_TTL);
    if provisioned {
        *slot = None;
        if was_missing {
            info!("Diiisco: device wallet is now provisioned");
        }
    } else {
        *slot = Some(Instant::now());
        if !was_missing {
            info!("Diiisco unavailable: {}", WALLET_NOT_PROVISIONED);
        }
    }
}

/// True while a recent lookup proved the device has no Algorand wallet.
/// Expires on its own so a provisioned wallet is retried without a restart.
pub fn wallet_missing() -> bool {
    let slot = match NO_WALLET_SINCE.lock() {
        Ok(s) => s,
        Err(poisoned) => poisoned.into_inner(),
    };
    cache_is_live(*slot, Instant::now(), WALLET_CACHE_TTL)
}

/// The `check_requirements` answer for a given wallet state. Split out so it
/// can be tested without constructing a `DiiiscoIntegration`, which needs a
/// live `ApiClient` and `ConfigStore`.
pub fn requirements_for(wallet_is_missing: bool) -> Result<(), String> {
    if wallet_is_missing {
        return Err(WALLET_NOT_PROVISIONED.to_string());
    }
    Ok(())
}
/// Bearer token resolved from the hardwareapi credential response, cached for
/// `health_check`, which has no credentials in hand. Never compiled in.
static CRED_BEARER: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// PURE: runtime env var (non-empty, dev override) -> credential response
/// field (non-empty) -> None.
fn resolve_diiisco_bearer(
    env: Option<String>,
    creds: &crate::api::types::CredentialInfo,
) -> Option<String> {
    env.filter(|s| !s.is_empty())
        .or_else(|| creds.diiisco_bearer_token.clone().filter(|s| !s.is_empty()))
}

/// Resolve the token or SKIP: one warning (no token value), then an error
/// through the caller's existing failure path. No docker build is attempted.
fn require_diiisco_bearer(creds: &crate::api::types::CredentialInfo) -> Result<String> {
    match resolve_diiisco_bearer(std::env::var("DIIISCO_BEARER_TOKEN").ok(), creds) {
        Some(t) => {
            if let Ok(mut g) = CRED_BEARER.lock() {
                *g = Some(t.clone());
            }
            Ok(t)
        }
        None => {
            warn!("Diiisco bearer token unavailable (no env override, none in credential response) — skipping Diiisco");
            anyhow::bail!("DIIISCO_BEARER_TOKEN not configured — set the environment variable before enabling Diiisco")
        }
    }
}

/// Token for the health probe: runtime env var, else the last one seen in a
/// credential response, else this device's own node key, else empty.
fn diiisco_bearer_token() -> String {
    std::env::var("DIIISCO_BEARER_TOKEN")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| CRED_BEARER.lock().ok().and_then(|g| g.clone()))
        .or_else(|| {
            deploy_dir()
                .ok()
                .and_then(|d| diiisco_node_key::read_key(&d))
        })
        .unwrap_or_default()
}

pub struct DiiiscoIntegration {
    pub api_client: Arc<ApiClient>,
    pub config: Arc<ConfigStore>,
}

/// PURE seam: given what `dirs::data_local_dir()` returned, resolve Diiisco's
/// deploy directory — or explain why it can't, instead of panicking.
///
/// Split out from `deploy_dir()` so "no local data dir" (in practice
/// unreachable on a dev machine, where $HOME is always set) is something a
/// test can DRIVE with a constructed `None`, rather than something only a
/// machine with no resolvable home directory could ever exercise.
fn resolve_local_deploy_dir(local_data_dir: Option<PathBuf>) -> Result<PathBuf> {
    // B13: this used to resolve `dirs::data_local_dir()` directly and never
    // consult the storage root, so a user who moved storage left this
    // deployment stranded at the old location. An existing legacy directory is
    // grandfathered, so nothing live moves and no migration is needed.
    let base = local_data_dir.ok_or_else(|| {
        anyhow::anyhow!(
            "Could not determine this OS's local application data directory — \
             Diiisco cannot resolve where to deploy"
        )
    })?;
    Ok(super::download::resolve_deploy_dir(
        base.join("FryEdgeMiner").join("diiisco"),
        super::download::partners_base_dir().join("diiisco"),
    ))
}

/// diiisco panic fix: a missing OS data directory used to `.expect()` here,
/// which unwound through every caller of `deploy_dir()`/`compose_file()` —
/// `install()`, `start()`, `stop()`, `installed_version()`,
/// `collect_poc_data()` — none of which could recover from a panic. Now a
/// handled `Result`; each caller surfaces or absorbs it per its own contract
/// (`Integration`'s trait methods can't all return `Result`).
fn deploy_dir() -> Result<PathBuf> {
    resolve_local_deploy_dir(dirs::data_local_dir())
}

/// Fetch credentials with exponential-backoff retry on network errors.
/// Retries up to 3 times (2s, 4s, 8s delay) for connection-level failures
/// only — HTTP error responses are NOT retried.
async fn fetch_credentials_with_retry(
    api_client: &ApiClient,
    miner_key: &str,
) -> std::result::Result<CredentialInfo, crate::api::client::ApiError> {
    let mut result = crate::api::credentials::lookup(api_client, miner_key).await;
    for attempt in 1..=3u32 {
        match &result {
            Err(crate::api::client::ApiError::Request(_)) => {
                let delay = 2u64.pow(attempt);
                warn!(
                    attempt,
                    delay_secs = delay,
                    "Credential fetch failed (network error), retrying"
                );
                tokio::time::sleep(Duration::from_secs(delay)).await;
                result = crate::api::credentials::lookup(api_client, miner_key).await;
            }
            _ => break,
        }
    }
    result
}

/// User-facing form of a failed credentials lookup. Network-level failures
/// route through the shared classifier (DNS / connect / timeout / TLS get
/// actionable guidance — carriers blocking *.frynetworks.com is a known
/// case); HTTP errors keep ApiError's already-sanitized display.
fn credential_fetch_error(e: crate::api::client::ApiError) -> anyhow::Error {
    use crate::api::error_classify::{classify_request_error, user_facing_message};
    if let crate::api::client::ApiError::Request(req) = &e {
        if let Some(msg) = user_facing_message(classify_request_error(req)) {
            return anyhow::anyhow!("Failed to fetch credentials — {msg}");
        }
    }
    anyhow::anyhow!("Failed to fetch credentials: {e}")
}

fn docker_available() -> bool {
    super::docker_manager::docker_cli_probe_bounded() == Some(true)
}

fn compose_file() -> Result<PathBuf> {
    Ok(deploy_dir()?.join("docker-compose.yml"))
}

/// The diiisco-node image is built locally by install(); it is never
/// published to a registry, so `compose up` must not be allowed to fall back
/// to pulling it.
///
/// Tri-state: a failed probe (spawn error, timeout, non-zero exit) is `Unknown`,
/// never "absent".
fn image_built() -> diiisco_rebuild::ImageState {
    use diiisco_rebuild::ImageState;
    match crate::integrations::docker_manager::docker_command()
        .args(["images", "-q", "diiisco-node:latest"])
        .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
    {
        Ok(o) if o.status.success() => {
            if String::from_utf8_lossy(&o.stdout).trim().is_empty() {
                ImageState::Absent
            } else {
                ImageState::Present
            }
        }
        _ => ImageState::Unknown,
    }
}

/// Last `n` lines of subprocess output — full walls of Docker logs go to the
/// log file, not the UI error banner.
fn tail_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

#[async_trait]
impl Integration for DiiiscoIntegration {
    fn id(&self) -> &str {
        "diiisco"
    }

    fn display_name(&self) -> &str {
        "Diiisco"
    }

    /// B3: a user who clicked the toggle may be asked to approve the Docker
    /// Desktop install once. Satisfying Docker first with that authority leaves
    /// the ordinary install path below untouched — `ensure_docker` is
    /// idempotent, so its call inside `install()` then finds Docker ready.
    /// Every other caller of `install()` (boot recovery, the Docker watcher)
    /// keeps getting Automatic, which the gate refuses without prompting.
    async fn install_for_user(&self) -> Result<()> {
        super::docker_manager::ensure_docker_with(
            crate::elevation_gate::ElevationTrigger::UserClick,
        )
        .await?;
        self.install().await
    }

    async fn install(&self) -> Result<()> {
        // B6: a recent lookup already proved this device has no wallet. Fail
        // immediately with the actionable reason rather than re-asking
        // hardwareapi and re-running a Docker build that cannot succeed.
        if wallet_missing() {
            anyhow::bail!("{}", WALLET_NOT_PROVISIONED);
        }
        // Ensure Docker is available, auto-installing if needed
        super::docker_manager::ensure_docker().await?;

        // Was an inline duplicate of `deploy_dir()`, which meant the two could
        // disagree about where Diiisco lives.
        let deploy_dir = deploy_dir()?;

        // Write Docker files from embedded content
        let node_dir = deploy_dir.join("diiisco-node");
        tokio::fs::create_dir_all(&node_dir).await?;
        tokio::fs::write(
            deploy_dir.join("docker-compose.yml"),
            include_str!("diiisco_deploy/docker-compose.yml"),
        )
        .await?;
        tokio::fs::write(
            node_dir.join("Dockerfile"),
            include_str!("diiisco_deploy/diiisco-node/Dockerfile"),
        )
        .await?;

        // Fetch credentials from hardwareapi
        let cfg = self.config.get();
        let miner_key = cfg.miner_key.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "Miner key not set — complete device registration before installing Diiisco"
            )
        })?;
        let mut creds = fetch_credentials_with_retry(&self.api_client, miner_key)
            .await
            .map_err(credential_fetch_error)?;
        let algo_address = match classify_wallet(creds.algo_address.as_deref()) {
            Ok(addr) => {
                record_wallet_state(true);
                addr
            }
            Err(reason) => {
                record_wallet_state(false);
                anyhow::bail!("{}", reason);
            }
        };
        let algo_mnemonic = creds.algo_mnemonic.clone().ok_or_else(|| {
            anyhow::anyhow!("Device Algorand mnemonic unavailable — contact support")
        })?;

        // Docker compose build with credentials as env vars (NOT command-line args)
        diiisco_node_key::fill_local_key(
            &mut creds,
            std::env::var("DIIISCO_BEARER_TOKEN").ok(),
            &deploy_dir,
        )?;
        let bearer = require_diiisco_bearer(&creds)?;
        let key_fp = diiisco_rebuild::fp12(&bearer);
        let pin = diiisco_rebuild::pin_for_tag();
        let tag = diiisco_rebuild::image_tag(&key_fp, &pin);
        diiisco_rebuild::require_pin_when_enabled(diiisco_rebuild::REBUILD_ENABLED, &pin)?;
        // Same lock as start(): a build never overlaps another build or switch.
        let _rebuild_guard = diiisco_rebuild::REBUILD_LOCK.lock().await;
        let state = image_built();
        match diiisco_rebuild::install_step(
            state,
            diiisco_rebuild::stamp_is_current(&deploy_dir, &key_fp, &pin),
        ) {
            diiisco_rebuild::InstallStep::Build => {}
            diiisco_rebuild::InstallStep::AlreadyBuilt => {
                info!("Diiisco image already built by a concurrent caller");
                return Ok(());
            }
            diiisco_rebuild::InstallStep::Unknown => {
                diiisco_rebuild::warn_image_unknown();
                anyhow::bail!("Diiisco image state could not be determined; not building")
            }
        }
        // A failed attempt for this target backs off: the startup recovery and
        // the Docker watcher call install() again on every cycle.
        let now = diiisco_rebuild::unix_now();
        diiisco_rebuild::install_backoff_check(&deploy_dir, &tag, now)?;
        let have_image = state == diiisco_rebuild::ImageState::Present;
        let node = DockerNode::new(
            deploy_dir.clone(),
            algo_address.clone(),
            algo_mnemonic.clone(),
            bearer.clone(),
            diiisco_rebuild::stop_generation(),
        );
        let prev_id = if have_image {
            diiisco_rebuild::NodeOps::image_lookup(&node, diiisco_rebuild::LATEST_REF).into_id()
        } else {
            None
        };
        info!("Building Diiisco Docker image");
        let built = node_cmd(
            &deploy_dir,
            &algo_address,
            &algo_mnemonic,
            &bearer,
            Some(&tag),
        )
        .args(["compose", "build"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output_bounded(crate::supervisor::platform::LONG_TIMEOUT);
        let output = match built {
            Ok(o) => o,
            Err(e) => {
                record_install_attempt(&deploy_dir, &tag);
                diiisco_rebuild::NodeOps::discard_candidate(&node, &tag);
                return Err(e.into());
            }
        };

        if !output.status.success() {
            let stderr = redact_out(
                &String::from_utf8_lossy(&output.stderr),
                &bearer,
                &algo_mnemonic,
                &deploy_dir,
            );
            warn!(stderr = %stderr, "docker compose build failed (full output)");
            record_install_attempt(&deploy_dir, &tag);
            diiisco_rebuild::NodeOps::discard_candidate(&node, &tag);
            anyhow::bail!("docker compose build failed: {}", tail_lines(&stderr, 15));
        }
        // Recorded only once the build has finished (cleared on success), so an
        // install dropped by the outer timeout mid-build leaves no backoff,
        // while a failed build or a failed F2 gate blocks re-entry.
        // The candidate's id goes on the ledger the sweep works from.
        let cand_id = diiisco_rebuild::NodeOps::record_candidate(&node, &tag);
        let prior_backoff = diiisco_rebuild::read_backoff(&deploy_dir);
        if let Err(e) =
            diiisco_rebuild::record_attempt(&deploy_dir, &tag, diiisco_rebuild::unix_now())
        {
            diiisco_rebuild::NodeOps::discard_candidate(&node, &tag);
            anyhow::bail!("cannot record the rebuild attempt: {e}");
        }
        // F2: the candidate runs and proves the device key before it becomes
        // `latest`; a rejected one is removed and the previous node restored.
        match diiisco_rebuild::gate_candidate(&node, &tag, have_image).await {
            diiisco_rebuild::Gate::Pass => {}
            diiisco_rebuild::Gate::Rejected => anyhow::bail!(
                "the new Diiisco node did not prove the device key; the previous node image was kept"
            ),
            diiisco_rebuild::Gate::Stopped => {
                // A user stop is not a failed attempt and promoted nothing:
                // no backoff, and callers must not treat this as installed.
                diiisco_rebuild::rollback_attempt(&deploy_dir, prior_backoff);
                anyhow::bail!("Diiisco install was stopped; nothing was promoted")
            }
        }
        // `latest` is the single moving alias for the current-good image; tag
        // it BEFORE the stamp so an env-less recovery `up` finds it.
        if let Err(e) = diiisco_rebuild::after_fresh_build(&node, &deploy_dir, &tag, &key_fp, &pin)
        {
            // `latest` may already point at the candidate: restore it first so
            // the previous node comes back and the candidate can be removed.
            diiisco_rebuild::rollback_latest(&node, prev_id.as_deref());
            diiisco_rebuild::reject_candidate(&node, &tag, have_image);
            return Err(e);
        }
        diiisco_rebuild::drop_previous(&node, prev_id.as_deref(), &tag);
        diiisco_rebuild::sweep_stale_candidates(&node, &deploy_dir, cand_id.as_deref());
        info!("Diiisco install complete — image built with per-device wallet");
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        let stop_gen = diiisco_rebuild::stop_generation();
        // B6: short-circuit BEFORE any API call. The supervisor retries start()
        // on a schedule; without this, a device waiting on provisioning issues
        // a credentials lookup every single attempt to be told the same thing.
        if wallet_missing() {
            anyhow::bail!("{}", WALLET_NOT_PROVISIONED);
        }
        super::docker_manager::ensure_docker().await?;
        // Resolved once here (rather than at each of the two spots that used
        // to call `deploy_dir()` separately) so start() can't disagree with
        // itself about where Diiisco lives partway through.
        let deploy = deploy_dir()?;
        let compose = deploy.join("docker-compose.yml");
        if !compose.exists() {
            anyhow::bail!(
                "Diiisco is not installed yet (deploy directory missing at {}) — toggle it off and on to reinstall",
                deploy.display()
            );
        }

        // Refresh the compose from the embedded copy on every start. Only
        // install() used to write it, and installed_version() reports installed
        // whenever the file exists — so a machine that installed an older build
        // would keep its stale compose forever and never pick up fixes like
        // dropping the conflicting 11434 publish.
        tokio::fs::write(&compose, include_str!("diiisco_deploy/docker-compose.yml")).await?;
        // Same for the Dockerfile: it is rewritten from the embedded copy here and
        // again before every build, so any build (a missing image, or the
        // enabled upgrade rebuild) uses the pinned source this binary carries.
        diiisco_rebuild::write_embedded_dockerfile(&deploy)?;

        // The compose file interpolates ${ALGO_ADDRESS}/${ALGO_MNEMONIC}/
        // ${DIIISCO_API_KEY} on EVERY invocation, `up` included — without
        // them compose warns "variable is not set" and passes blank build
        // args. Fetch the same credentials install() uses.
        let cfg = self.config.get();
        let miner_key = cfg.miner_key.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "Miner key not set — complete device registration before starting Diiisco"
            )
        })?;
        let mut creds = fetch_credentials_with_retry(&self.api_client, miner_key)
            .await
            .map_err(credential_fetch_error)?;
        let algo_address = match classify_wallet(creds.algo_address.as_deref()) {
            Ok(addr) => {
                record_wallet_state(true);
                addr
            }
            Err(reason) => {
                record_wallet_state(false);
                anyhow::bail!("{}", reason);
            }
        };
        let algo_mnemonic = creds.algo_mnemonic.clone().ok_or_else(|| {
            anyhow::anyhow!("Device Algorand mnemonic unavailable — contact support")
        })?;
        diiisco_node_key::fill_local_key(
            &mut creds,
            std::env::var("DIIISCO_BEARER_TOKEN").ok(),
            &deploy,
        )?;
        let bearer = require_diiisco_bearer(&creds)?;

        // diiisco-node is built locally, never pulled. The plan, the build and
        // the switch run detached: `toggle_integration` bounds start() at 60 s
        // but a build cannot be preempted, so a dropped start() must not
        // abandon a half-done switch. A stop() in the meantime wins (see
        // `stop_generation`).
        let node = DockerNode::new(
            deploy.clone(),
            algo_address,
            algo_mnemonic,
            bearer.clone(),
            stop_gen,
        );
        tokio::spawn(run_node(node, deploy, bearer))
            .await
            .map_err(|e| anyhow::anyhow!("Diiisco start task failed: {e}"))?
    }

    async fn stop(&self) -> Result<()> {
        // A detached start sequence that began earlier must not bring the node
        // back up after this.
        diiisco_rebuild::bump_stop_generation();
        // An unresolvable deploy directory means there is nothing deployed to
        // stop — the same "nothing to do" outcome as a missing compose file
        // below, not a failure to report.
        let Ok(compose) = compose_file() else {
            return Ok(());
        };
        if compose.exists() {
            crate::integrations::docker_manager::docker_command()
                .args(["compose", "-f", &compose.to_string_lossy(), "stop"])
                .env_remove("DIIISCO_IMAGE_TAG")
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)?;
            info!("Stopped Diiisco containers");
        }
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        if !docker_available() {
            // State-aware guidance ("not installed" vs "not running" vs
            // VM/BIOS virtualization) instead of a generic string.
            return HealthStatus::Unhealthy(super::docker_manager::status_user_message(
                super::docker_manager::docker_status(),
            ));
        }
        let token = diiisco_bearer_token();
        if token.is_empty() {
            warn!("No DIIISCO_BEARER_TOKEN set — health check may fail");
        }
        let client = reqwest::Client::new();
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            client.get(HEALTH_URL).bearer_auth(token).send(),
        )
        .await
        {
            Ok(Ok(resp)) if resp.status().is_success() => HealthStatus::Healthy,
            Ok(Ok(resp)) => HealthStatus::Unhealthy(format!("HTTP {}", resp.status())),
            Ok(Err(_)) => HealthStatus::Stopped, // connection refused = not running
            Err(_) => HealthStatus::Unhealthy("Timeout".to_string()),
        }
    }

    async fn check_update(&self) -> Result<Option<String>> {
        Ok(None) // Docker images auto-update via pull
    }

    async fn apply_update(&self, _version: &str) -> Result<()> {
        self.install().await // re-pull latest images
    }

    fn installed_version(&self) -> Option<String> {
        // An unresolvable deploy directory can't hold an install.
        let Ok(compose) = compose_file() else {
            return None;
        };
        if !compose.exists() {
            return None;
        }
        // Compose file present but image never built = failed/interrupted
        // install — report not-installed so the toggle path re-runs install.
        // Only checkable when the Docker engine is reachable. A probe that
        // itself failed (Unknown) is NOT "not installed": that would trigger an
        // install on a glitch.
        if docker_available() && image_built() == diiisco_rebuild::ImageState::Absent {
            return None;
        }
        Some("installed".into())
    }

    fn collect_poc_data(&self) -> PocGateData {
        // Synchronous check — can't do async health here, check compose file existence
        let compose_exists = compose_file().map(|p| p.exists()).unwrap_or(false);
        PocGateData {
            poa: compose_exists && docker_available(),
            ..Default::default()
        }
    }

    fn requires_docker(&self) -> bool {
        true
    }

    /// B6: a device whose wallet has not been provisioned yet cannot run
    /// Diiisco, and saying so here is what makes the card inert (with the
    /// reason) instead of leaving the user staring at a red start error the
    /// health loop keeps re-triggering. Self-clearing: the next successful
    /// credentials lookup resets the flag.
    fn check_requirements(&self) -> Result<(), String> {
        requirements_for(wallet_missing())
    }
}

/// Compose/docker command for the diiisco project. Compose interpolates the
/// wallet and key variables on EVERY invocation; the image tag is set only when
/// a specific tag is intended and removed otherwise, so a stray process env can
/// never redirect compose.
fn node_cmd(
    deploy: &std::path::Path,
    algo_address: &str,
    algo_mnemonic: &str,
    bearer: &str,
    tag: Option<&str>,
) -> std::process::Command {
    let mut c = crate::integrations::docker_manager::docker_command();
    c.env("ALGO_ADDRESS", algo_address)
        .env("ALGO_MNEMONIC", algo_mnemonic)
        .env("DIIISCO_API_KEY", bearer)
        .current_dir(deploy);
    match tag {
        Some(t) => {
            c.env("DIIISCO_IMAGE_TAG", t);
        }
        None => {
            c.env_remove("DIIISCO_IMAGE_TAG");
        }
    }
    c
}

/// Best-effort record of a failed install attempt for `tag`.
fn record_install_attempt(deploy: &std::path::Path, tag: &str) {
    if let Err(e) = diiisco_rebuild::record_attempt(deploy, tag, diiisco_rebuild::unix_now()) {
        warn!(error = %e, "Diiisco: cannot record the failed install attempt");
    }
}

/// Docker output with the bearer and the device key masked, for any log line
/// or error text.
fn redact_out(s: &str, bearer: &str, algo_mnemonic: &str, deploy: &std::path::Path) -> String {
    let device = diiisco_node_key::read_key(deploy).unwrap_or_default();
    diiisco_rebuild::redact_node_output(s, &[bearer, device.as_str(), algo_mnemonic])
}

const PROBE_URL: &str = "http://127.0.0.1:8181/v1/models";

async fn http_status(rb: reqwest::RequestBuilder) -> u16 {
    rb.send().await.map(|r| r.status().as_u16()).unwrap_or(0)
}

/// The Docker side of start(): one deploy dir plus the credentials compose
/// interpolates. Implements the seam the rebuild sequence is tested through.
struct DockerNode {
    deploy: PathBuf,
    algo_address: String,
    algo_mnemonic: String,
    bearer: String,
    stop_gen: u64,
}

impl DockerNode {
    fn new(
        deploy: PathBuf,
        algo_address: String,
        algo_mnemonic: String,
        bearer: String,
        stop_gen: u64,
    ) -> Self {
        DockerNode {
            deploy,
            algo_address,
            algo_mnemonic,
            bearer,
            stop_gen,
        }
    }

    fn node_cmd(&self, tag: Option<&str>) -> std::process::Command {
        node_cmd(
            &self.deploy,
            &self.algo_address,
            &self.algo_mnemonic,
            &self.bearer,
            tag,
        )
    }

    fn redact(&self, s: &str) -> String {
        redact_out(s, &self.bearer, &self.algo_mnemonic, &self.deploy)
    }

    /// The single `docker rmi <id>` call site. `what` names the image in the log.
    fn rmi_logged(&self, id: &str, what: &str) {
        match self.docker(&["rmi", id]) {
            Ok(o) if o.status.success() => {
                info!("{}", diiisco_rebuild::rmi_log_text(what, true))
            }
            Ok(o) => warn!(
                stderr = %self.redact(&String::from_utf8_lossy(&o.stderr)),
                "{}",
                diiisco_rebuild::rmi_log_text(what, false)
            ),
            Err(e) => warn!(error = %e, "{}", diiisco_rebuild::rmi_log_text(what, false)),
        }
    }

    fn docker(&self, args: &[&str]) -> std::io::Result<std::process::Output> {
        crate::integrations::docker_manager::docker_command()
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
    }

    fn run_up(&self, cmd: &mut std::process::Command, what: &str) -> Result<()> {
        info!("{what}");
        let output = cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output_bounded(crate::supervisor::platform::LONG_TIMEOUT)?;
        if !output.status.success() {
            let stderr = self.redact(&String::from_utf8_lossy(&output.stderr));
            warn!(stderr = %stderr, "Failed to start Diiisco (full output)");
            anyhow::bail!("Failed to start Diiisco: {}", tail_lines(&stderr, 15));
        }
        Ok(())
    }
}

#[async_trait]
impl diiisco_rebuild::NodeOps for DockerNode {
    fn image_exists(&self) -> bool {
        image_built() == diiisco_rebuild::ImageState::Present
    }

    fn image_state(&self) -> diiisco_rebuild::ImageState {
        image_built()
    }

    fn refresh_dockerfile(&self) -> Result<()> {
        diiisco_rebuild::write_embedded_dockerfile(&self.deploy)?;
        Ok(())
    }

    fn image_id(&self, reference: &str) -> Option<String> {
        diiisco_rebuild::NodeOps::image_lookup(self, reference).into_id()
    }

    /// A failed command (spawn, timeout, non-zero exit) is `Error`, never an
    /// absent image.
    fn image_lookup(&self, reference: &str) -> diiisco_rebuild::ImageLookup {
        use diiisco_rebuild::ImageLookup;
        let Ok(out) = self.docker(&["images", "-q", "--no-trunc", reference]) else {
            return ImageLookup::Error;
        };
        if !out.status.success() {
            return ImageLookup::Error;
        }
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map_or(ImageLookup::Absent, |id| {
                ImageLookup::Present(id.to_string())
            })
    }

    fn build(&self, tag: &str) -> Result<()> {
        let output = self
            .node_cmd(Some(tag))
            .args(["compose", "build"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output_bounded(crate::supervisor::platform::LONG_TIMEOUT)?;
        if !output.status.success() {
            let stderr = self.redact(&String::from_utf8_lossy(&output.stderr));
            warn!(stderr = %stderr, "Diiisco pre-start image build failed (full output)");
            anyhow::bail!("Diiisco image build failed: {}", tail_lines(&stderr, 15));
        }
        Ok(())
    }

    fn down(&self) {
        // Best-effort cleanup of stale containers/networks from prior failed
        // runs ("network diiisco_default already exists"). Volumes survive.
        // Failures don't block startup, but they must be visible in logs.
        info!("Cleaning up stale Diiisco containers/networks");
        match self
            .node_cmd(None)
            .args(["compose", "down", "--remove-orphans"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
        {
            Ok(o) if !o.status.success() => {
                warn!(
                    stderr = %self.redact(&String::from_utf8_lossy(&o.stderr)),
                    "Diiisco pre-start cleanup reported errors (continuing)"
                );
            }
            Err(e) => {
                warn!(error = %e, "Diiisco pre-start cleanup failed to run (continuing)");
            }
            _ => {}
        }
    }

    fn up_plain(&self) -> Result<()> {
        self.run_up(
            self.node_cmd(None).args(["compose", "up", "-d"]),
            "Starting Diiisco containers",
        )
    }

    fn up_no_build(&self, tag: Option<&str>) -> Result<()> {
        self.run_up(
            self.node_cmd(tag)
                .args(["compose", "up", "-d", "--no-build"]),
            "Starting Diiisco containers without building",
        )
    }

    /// F2: the node must reject a request with no credentials at all (401) AND
    /// let the device key past auth (any real status except 401/403; a 500 "No
    /// peers available" from an offline mesh counts). Each request is bounded
    /// to 5 s; retried while the node boots.
    async fn probe(&self) -> bool {
        let Ok(client) = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .no_proxy()
            .build()
        else {
            return false;
        };
        let rounds = diiisco_rebuild::PROBE_BUDGET_SECS / diiisco_rebuild::PROBE_STEP_SECS;
        for round in 0..=rounds {
            if self.stop_requested() {
                return false;
            }
            let with_key = http_status(client.get(PROBE_URL).bearer_auth(&self.bearer)).await;
            let no_key = http_status(client.get(PROBE_URL)).await;
            if diiisco_rebuild::probe_verdict(with_key, no_key) {
                return true;
            }
            if round < rounds {
                tokio::time::sleep(Duration::from_secs(diiisco_rebuild::PROBE_STEP_SECS)).await;
            }
        }
        false
    }

    fn tag_latest(&self, tag: &str) -> Result<()> {
        let source = format!("diiisco-node:{tag}");
        let out = self.docker(&["tag", source.as_str(), diiisco_rebuild::LATEST_REF])?;
        if !out.status.success() {
            let stderr = self.redact(&String::from_utf8_lossy(&out.stderr));
            anyhow::bail!("docker tag failed: {}", tail_lines(&stderr, 5));
        }
        Ok(())
    }

    /// `docker ps -aq`, then `docker inspect --format {{.Image}} <ids>`: the
    /// image ID of EVERY container, stopped ones included. Any failure = `None`.
    fn all_container_image_ids(&self) -> Option<Vec<String>> {
        let ps = self.docker(&["ps", "-aq"]).ok()?;
        if !ps.status.success() {
            return None;
        }
        let ids = non_empty_lines(&ps.stdout);
        if ids.is_empty() {
            return Some(ids);
        }
        let mut args = vec!["inspect", "--format", "{{.Image}}"];
        args.extend(ids.iter().map(String::as_str));
        let out = self.docker(&args).ok()?;
        if !out.status.success() {
            return None;
        }
        Some(non_empty_lines(&out.stdout))
    }

    fn list_node_images(&self) -> Option<Vec<diiisco_rebuild::ImageRef>> {
        let out = self
            .docker(&[
                "images",
                "diiisco-node",
                "--no-trunc",
                "--format",
                "{{.ID}} {{.Tag}}",
            ])
            .ok()?;
        out.status
            .success()
            .then(|| diiisco_rebuild::parse_image_listing(&String::from_utf8_lossy(&out.stdout)))
    }

    fn dangling_image_count(&self) -> Option<usize> {
        let out = self
            .docker(&["images", "--filter", "dangling=true", "-q", "--no-trunc"])
            .ok()?;
        out.status
            .success()
            .then(|| non_empty_lines(&out.stdout).len())
    }

    fn image_built_by_fem(&self, id: &str) -> bool {
        self.docker(&["history", "--no-trunc", "--format", "{{.CreatedBy}}", id])
            .is_ok_and(|o| {
                o.status.success()
                    && diiisco_rebuild::history_is_fem(&String::from_utf8_lossy(&o.stdout))
            })
    }

    fn record_candidate(&self, tag: &str) -> Option<String> {
        let id = self
            .image_lookup(&format!("diiisco-node:{tag}"))
            .into_id()?;
        if let Err(e) = diiisco_rebuild::ledger_add(&self.deploy, &id) {
            warn!(error = %e, "Diiisco: could not record the candidate image id");
        }
        Some(id)
    }

    fn forget_candidate(&self, id: &str) {
        diiisco_rebuild::ledger_remove(&self.deploy, id);
    }

    fn stamp_tag(&self) -> Option<String> {
        diiisco_rebuild::read_stamp(&self.deploy).map(|s| s.tag)
    }

    fn retag_alias(&self, tag: &str, id: &str) -> Result<()> {
        let out = self.docker(&["tag", id, &format!("diiisco-node:{tag}")])?;
        if !out.status.success() {
            let stderr = self.redact(&String::from_utf8_lossy(&out.stderr));
            anyhow::bail!("docker tag failed: {}", tail_lines(&stderr, 5));
        }
        Ok(())
    }

    fn retag_latest(&self, id: &str) -> Result<()> {
        let out = self.docker(&["tag", id, diiisco_rebuild::LATEST_REF])?;
        if !out.status.success() {
            let stderr = self.redact(&String::from_utf8_lossy(&out.stderr));
            anyhow::bail!("docker tag failed: {}", tail_lines(&stderr, 5));
        }
        Ok(())
    }

    fn rmi_by_id(&self, id: &str) {
        self.rmi_logged(id, "image")
    }

    fn rmi_labeled(&self, id: &str, what: &str) {
        self.rmi_logged(id, what)
    }

    fn discard_candidate(&self, tag: &str) {
        diiisco_rebuild::discard_candidate_by_id(self, tag)
    }

    /// `docker ps -q`, then `docker inspect --format {{.Image}} <ids>`: the image
    /// ID of every running container. Any Docker failure is `None`.
    fn running_image_ids(&self) -> Option<Vec<String>> {
        let ps = self.docker(&["ps", "-q"]).ok()?;
        if !ps.status.success() {
            return None;
        }
        let ids = non_empty_lines(&ps.stdout);
        if ids.is_empty() {
            return Some(ids);
        }
        let mut args = vec!["inspect", "--format", "{{.Image}}"];
        args.extend(ids.iter().map(String::as_str));
        let out = self.docker(&args).ok()?;
        if !out.status.success() {
            return None;
        }
        Some(non_empty_lines(&out.stdout))
    }

    fn stop_requested(&self) -> bool {
        diiisco_rebuild::stop_generation() != self.stop_gen
    }
}

fn non_empty_lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// The detached half of start(): plan, build, switch, run.
async fn run_node(node: DockerNode, deploy: PathBuf, bearer: String) -> Result<()> {
    let key_fp = diiisco_rebuild::fp12(&bearer);
    let pin = diiisco_rebuild::pin_for_tag();
    let outcome = diiisco_rebuild::run_node_sequence(
        &node,
        &deploy,
        &key_fp,
        &pin,
        diiisco_rebuild::unix_now,
        diiisco_rebuild::REBUILD_ENABLED,
    )
    .await?;
    if !matches!(
        outcome,
        diiisco_rebuild::Outcome::Stopped | diiisco_rebuild::Outcome::ImageUnknown
    ) {
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    const COMPOSE: &str = include_str!("diiisco_deploy/docker-compose.yml");

    #[test]
    fn ollama_publishes_no_host_port() {
        // Publishing 11434 collided with any Ollama the user already runs and
        // killed `docker compose up` with a bind error. Nothing outside the
        // compose network needs it.
        assert!(
            !COMPOSE.contains("11434:11434"),
            "the ollama host port publish must stay removed"
        );
    }

    #[test]
    fn ollama_is_still_reachable_inside_the_compose_network() {
        // Removing the publish is only safe while consumers use service DNS.
        assert!(COMPOSE.contains("http://ollama:11434"));
    }

    #[test]
    fn diiisco_still_publishes_its_api_port() {
        // health_check() hits http://localhost:8181/health from the host, so
        // this publish is required.
        assert!(COMPOSE.contains("8181:8181"));
    }

    #[test]
    fn ollama_service_and_healthcheck_survived_the_edit() {
        assert!(COMPOSE.contains("container_name: diiisco-ollama"));
        assert!(COMPOSE.contains(r#"["CMD", "ollama", "list"]"#));
    }
}

/// B6: an unprovisioned device wallet is a state to explain, not a crash.
#[cfg(test)]
mod wallet_condition_tests {
    use super::*;

    #[test]
    fn a_real_address_is_accepted_and_trimmed() {
        let addr = "TESTWALLETADDRESSTESTWALLETADDRESSTESTWALLETADDRESSTESTWA";
        assert_eq!(classify_wallet(Some(addr)).unwrap(), addr);
        assert_eq!(classify_wallet(Some("  ABC  ")).unwrap(), "ABC");
    }

    #[test]
    fn a_missing_address_maps_to_the_provisioning_message() {
        assert_eq!(classify_wallet(None), Err(WALLET_NOT_PROVISIONED));
    }

    #[test]
    fn a_blank_address_counts_as_not_provisioned() {
        // hardwareapi returns "" for a device row created before its wallet.
        assert_eq!(classify_wallet(Some("")), Err(WALLET_NOT_PROVISIONED));
        assert_eq!(classify_wallet(Some("   ")), Err(WALLET_NOT_PROVISIONED));
    }

    #[test]
    fn the_message_tells_the_user_where_to_go_first() {
        // The old text ("re-register or contact support") named no screen.
        assert!(WALLET_NOT_PROVISIONED.contains("Settings"));
        assert!(WALLET_NOT_PROVISIONED.contains("re-register"));
        // It must not read like a failure — that is what drove the support load.
        let lower = WALLET_NOT_PROVISIONED.to_lowercase();
        assert!(!lower.contains("failed"));
        assert!(!lower.contains("error"));
    }

    #[test]
    fn the_requirement_gate_reports_exactly_the_same_reason() {
        // Whatever the user sees as unavailable_reason has to be this string,
        // or the card and the toggle rejection disagree.
        assert_eq!(
            requirements_for(true),
            Err(WALLET_NOT_PROVISIONED.to_string())
        );
        assert_eq!(requirements_for(false), Ok(()));
    }

    #[test]
    fn the_observed_state_drives_the_gate_and_clears_itself() {
        // One test owns the process-wide cache so parallel siblings cannot
        // observe it mid-transition.
        record_wallet_state(false);
        assert!(wallet_missing());
        assert_eq!(
            requirements_for(wallet_missing()),
            Err(WALLET_NOT_PROVISIONED.to_string())
        );

        record_wallet_state(true);
        assert!(
            !wallet_missing(),
            "a provisioned wallet must clear the cache"
        );
        assert_eq!(requirements_for(wallet_missing()), Ok(()));
    }

    #[test]
    fn a_cached_miss_is_authoritative_only_inside_the_ttl() {
        let ttl = Duration::from_secs(900);
        let recorded = Instant::now();
        assert!(
            cache_is_live(Some(recorded), recorded, ttl),
            "just recorded"
        );
        assert!(cache_is_live(
            Some(recorded),
            recorded + Duration::from_secs(899),
            ttl
        ));
        // At the boundary the cache stops answering and the next attempt asks
        // hardwareapi again — that is what lets a provisioned wallet recover
        // without restarting the app.
        assert!(!cache_is_live(
            Some(recorded),
            recorded + Duration::from_secs(900),
            ttl
        ));
        assert!(!cache_is_live(
            Some(recorded),
            recorded + Duration::from_secs(5_000),
            ttl
        ));
    }

    #[test]
    fn no_cached_miss_never_blocks_a_lookup() {
        assert!(!cache_is_live(None, Instant::now(), WALLET_CACHE_TTL));
    }

    #[test]
    fn the_ttl_is_long_enough_to_stop_the_retry_storm() {
        // The supervisor re-arms roughly every 5 minutes; a TTL below that
        // would let the hammering resume.
        assert!(WALLET_CACHE_TTL >= Duration::from_secs(600));
    }
}

/// The credential-fetch card used to show reqwest's raw chain — full URL
/// included, truncated mid-word by the card. Network failures should say
/// what to do instead.
#[cfg(test)]
mod credential_error_message_tests {
    use super::*;

    #[tokio::test]
    async fn a_network_failure_reads_as_guidance_not_a_reqwest_chain() {
        // .invalid is reserved (RFC 2606) — resolution always fails locally.
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();
        let req_err = client
            .get("https://fem-cred-test.invalid/credentials/x")
            .send()
            .await
            .expect_err("request to .invalid must fail");
        let e = crate::api::client::ApiError::Request(req_err);
        let msg = credential_fetch_error(e).to_string();
        assert!(msg.starts_with("Failed to fetch credentials"), "{msg}");
        assert!(
            !msg.contains("error sending request for url"),
            "raw reqwest chain leaked: {msg}"
        );
        assert!(
            msg.contains("hardwareapi.frynetworks.com") || msg.contains("connection"),
            "message must be actionable: {msg}"
        );
    }

    #[test]
    fn an_http_status_error_keeps_its_sanitized_display() {
        let e = crate::api::client::ApiError::HttpStatus(504, "<html>x</html>".into());
        let msg = credential_fetch_error(e).to_string();
        assert!(msg.contains("504"), "{msg}");
        assert!(!msg.contains("<html>"), "{msg}");
    }
}

/// The diiisco panic fix: a missing local data dir used to `.expect()`.
/// Separate file so the inline `mod`s above stay byte-identical.
#[cfg(test)]
#[path = "diiisco_deploy_dir_tests.rs"]
mod diiisco_deploy_dir_tests;

#[cfg(test)]
#[path = "diiisco_cred_c9_tests.rs"]
mod diiisco_cred_c9_tests;

#[cfg(test)]
#[path = "diiisco_resolve_c9_tests.rs"]
mod diiisco_resolve_c9_tests;

#[cfg(test)]
#[path = "diiisco_health_tests.rs"]
mod diiisco_health_tests;
