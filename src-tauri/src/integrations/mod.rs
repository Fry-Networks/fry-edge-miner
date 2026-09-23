pub mod aem;
pub mod code_integrity;
pub mod diiisco;
pub mod docker_manager;
pub mod download;
pub mod firewall;
// filecoin_checker is retired: the Checker/Filecoin Station network is gone
// (repo archived 2025-06; checker.network, filstation.app, api.filspark.com and
// station-wallet-screening.fly.dev all NXDOMAIN). The module is kept on disk,
// unexported, so it can be restored by re-adding this line if the network returns.
// pub mod filecoin_checker;
pub mod fryvpn;
pub mod iagon;
pub mod mysterium;
pub mod mysterium_lan_check;
pub mod pawns;
pub mod port_conflict;
pub mod reward_model;
pub mod sentinel;
pub mod space_acres;
pub mod storj;
pub mod titan;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// --- Health & Lifecycle ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum HealthStatus {
    Healthy,
    Unhealthy(String),
    Stopped,
    Installing,
    Starting,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum LifecycleState {
    Disabled,
    Installing,
    Starting,
    Running,
    Unhealthy,
    Restarting,
    Failed,
    Stopping,
    Updating,
}

/// Who stands behind an integration.
///
/// `Official` partners are contracted by Fry Networks and carry the base
/// reward proportion. `Sdk` ones are community builds on the partner SDK:
/// experimental, and a bonus on top rather than a requirement. The UI splits
/// its Dashboard and Integrations screens on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntegrationTier {
    Official,
    Sdk,
}

/// Longest stderr excerpt allowed into a UI error banner.
const MAX_STDERR_CHARS: usize = 500;

/// The last `n` non-empty lines of a subprocess's stderr, collapsed to one line.
///
/// B4: callers used `stderr.lines().last()`, which keeps exactly one line. For
/// `docker run` that line is the generic usage hint rather than the cause, and
/// when the output ends in a whitespace-only line it is blank — so the user
/// got an error message that stopped at the colon. Blank lines are dropped,
/// the last `n` survivors are joined with " | " to fit the one-line card slot,
/// and the result is capped on a character boundary so a wall of Docker output
/// cannot flood the layout. Every call site already logs the untruncated text.
pub(crate) fn stderr_tail(stderr: &str, n: usize) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return "no error output".to_string();
    }
    let start = lines.len().saturating_sub(n);
    let joined = lines[start..].join(" | ");
    if joined.chars().count() > MAX_STDERR_CHARS {
        let head: String = joined.chars().take(MAX_STDERR_CHARS).collect();
        format!("{}…", head)
    } else {
        joined
    }
}

/// BUG 3/10: pure state check on a tracked child process. `Some(true)` =
/// confirmed still running, `Some(false)` = confirmed exited (caller should
/// clear the slot), `None` = nothing tracked, caller must fall back to an
/// image-name probe. Shared by SpaceAcres (BUG 3) and Olostep (BUG 10) —
/// both are spawned via a bare `Command::spawn()` with the `Child` handle
/// previously discarded, so neither integration could tell its OWN spawned
/// process apart from one it merely adopted (already running, started by
/// Windows autostart or a previous FEM session).
/// BUG 5 + BUG 8: does this unhealthy reason represent "waiting on a step only
/// the user can perform" rather than a fault the supervisor could recover from?
///
/// Restarting these accomplishes nothing and burns a stop/start cycle every
/// 30 s for as long as the integration is enabled. Matched on the reason text
/// because `HealthStatus` has no dedicated variant and adding one would ripple
/// into the TypeScript union and the LifecycleState mapping.
pub(crate) fn awaits_user_action(reason: &str) -> bool {
    AWAITING_MARKERS.iter().any(|m| reason.contains(m))
}

/// D-19: the UNION of every "waiting on a step only the user can take" marker
/// — B7/B8 (fryDVPN funding), B10 (a held TCP port), B15 (an OS code-integrity
/// refusal) and B17 (Iagon / Sentinel setup). None of these is a state the
/// supervisor can restart its way out of, so restarting through one only burns
/// the restart budget every 30 s and never changes the outcome.
///
/// Written as plain literals ON PURPOSE, even though three of them also exist
/// as constants next to the code that emits them: `src/lib/setupRequired.ts`
/// mirrors this list and `setupRequiredParity.test.ts` parses the array
/// literal out of this file, so a `module::CONST` here would be invisible to
/// it. `marker_parity_tests` below pins each literal against its source
/// constant, so the two cannot drift apart either way.
///
/// Adding a marker cannot change the verdict for any string that lacks it, so
/// every existing test stays green.
pub(crate) const AWAITING_MARKERS: [&str; 7] = [
    "Awaiting Storj setup",
    "needs your consent",
    "node token not provisioned",
    "node account not funded",
    "Awaiting fryDVPN funding",
    "Awaiting administrator action",
    "waiting for that program to release it",
];

/// B16: does this reason describe an UPSTREAM condition rather than a fault on
/// this device?
///
/// Distinct from `awaits_user_action` on purpose: nothing is being waited on
/// from the user, and the card still says something is wrong. What must NOT
/// happen is FEM killing a perfectly live partner process over a network
/// condition it cannot influence — titan-edge was being TerminateProcess'd
/// while running, purely because it had logged that it could not reach its
/// scheduler.
///
/// Keyed off the existing user-facing message so the two cannot drift.
pub(crate) fn upstream_unreachable(reason: &str) -> bool {
    const UPSTREAM_MARKERS: [&str; 1] = ["cannot reach the Titan scheduler"];
    UPSTREAM_MARKERS.iter().any(|m| reason.contains(m))
}

pub(crate) fn tracked_child_probe(slot: &mut Option<std::process::Child>) -> Option<bool> {
    match slot {
        None => None,
        Some(child) => match child.try_wait() {
            Ok(None) => Some(true),
            Ok(Some(_)) => {
                *slot = None;
                Some(false)
            }
            // A probe error on OUR OWN tracked child is exactly the
            // "unmeasurable" case an image-name probe already fails closed
            // on — assume still running rather than risking a double-spawn.
            Err(_) => Some(true),
        },
    }
}

/// Tier for a registered integration id. Static by design — the tier is a
/// commercial fact about the partner, not runtime state. Unknown ids are
/// `Sdk`: a new integration must be promoted deliberately, never by default.
pub fn tier_for(id: &str) -> IntegrationTier {
    match id {
        "mysterium" | "diiisco" | "space_acres" | "aem" | "fryvpn" => IntegrationTier::Official,
        _ => IntegrationTier::Sdk,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationStatus {
    pub id: String,
    pub display_name: String,
    pub enabled: bool,
    pub health: HealthStatus,
    pub lifecycle: LifecycleState,
    pub version: Option<String>,
    pub poc_contribution: f64,
    /// Official partner vs community SDK build (see `tier_for`).
    pub tier: IntegrationTier,
    #[serde(default)]
    pub requires_docker: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Set when `check_requirements()` fails: why this machine cannot run the
    /// integration. Presence is what marks a card auto-disabled in the UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

// --- PoC Gate Data ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PocGateData {
    pub data: bool,
    pub online: bool,
    pub mac_match: bool,
    pub pol: bool,
    pub poi: bool,
    pub poa: bool,
}

impl Default for PocGateData {
    fn default() -> Self {
        Self {
            data: true,
            online: true,
            mac_match: true,
            pol: true,
            poi: true,
            poa: true,
        }
    }
}

// --- Integration Trait ---

#[async_trait]
pub trait Integration: Send + Sync {
    fn id(&self) -> &str;
    fn display_name(&self) -> &str;
    async fn install(&self) -> Result<()>;
    /// Install because the USER just asked for it, as distinct from the boot
    /// recovery pass, the Docker watcher or an update step.
    ///
    /// B3: an install may need to run an elevated redistributable installer,
    /// and only a human gesture may raise a UAC prompt. Everything else goes
    /// through `install()` and gets `ElevationTrigger::Automatic`, which the
    /// gate refuses before any prompt is shown. Defaults to `install()`, so an
    /// integration whose install never elevates is unaffected.
    async fn install_for_user(&self) -> Result<()> {
        self.install().await
    }
    async fn start(&self) -> Result<()>;
    /// Start because the USER just asked for it, as distinct from a boot
    /// auto-start, a supervisor restart or the Docker watcher.
    ///
    /// B3: this is the ONLY thing that may raise a UAC prompt. Everything else
    /// goes through `start()` and gets `ElevationTrigger::Automatic`, which the
    /// gate refuses before any prompt is shown — so FEM cannot elevate without
    /// a human gesture. Defaults to `start()`, so an integration that never
    /// elevates is unaffected.
    async fn start_for_user(&self) -> Result<()> {
        self.start().await
    }
    async fn stop(&self) -> Result<()>;
    /// Stop this integration because the SUPERVISOR is recycling it, as
    /// distinct from the user turning it off.
    ///
    /// The owner's decision has not changed — only the process is being
    /// restarted — so an integration that keeps a durable record of the
    /// owner's consent must NOT record a withdrawal here. Defaults to `stop()`,
    /// so nothing changes for any integration that does not track consent.
    async fn stop_for_restart(&self) -> Result<()> {
        self.stop().await
    }
    /// Stop this integration because the USER disabled it, as distinct from
    /// the supervisor restarting it.
    ///
    /// Defaults to `stop()`, so no integration's behaviour changes. fryDVPN
    /// overrides it to ask frynode to deregister itself on the way out — an
    /// on-chain call that belongs to a deliberate disable and must NOT happen
    /// on every supervisor restart.
    async fn stop_for_disable(&self) -> Result<()> {
        self.stop().await
    }
    async fn health_check(&self) -> HealthStatus;
    async fn check_update(&self) -> Result<Option<String>>;
    async fn apply_update(&self, _version: &str) -> Result<()> {
        Ok(()) // default no-op
    }
    fn collect_poc_data(&self) -> PocGateData {
        PocGateData::default()
    }
    fn installed_version(&self) -> Option<String> {
        None
    }
    /// Whether this integration needs a running Docker engine. Drives
    /// availability display and prevents Docker auto-install at app boot.
    fn requires_docker(&self) -> bool {
        false
    }
    /// Whether this machine meets the partner network's published minimum
    /// specs. `Err(reason)` marks the integration unavailable: it is shown
    /// greyed out with `reason`, cannot be toggled on, and is excluded from
    /// the PoC proportion denominator so the user is not penalised for
    /// hardware they cannot run.
    ///
    /// Synchronous on purpose — `poc::reporter::build_poc_doc` is sync and
    /// needs the available count. Implementations must stay cheap; probe
    /// results are memoised in `crate::system_info`.
    fn check_requirements(&self) -> Result<(), String> {
        Ok(())
    }
}

// --- Registry ---

pub struct IntegrationRegistry {
    integrations: HashMap<String, Arc<dyn Integration>>,
    enabled: HashMap<String, bool>,
}

impl IntegrationRegistry {
    pub fn new() -> Self {
        Self {
            integrations: HashMap::new(),
            enabled: HashMap::new(),
        }
    }

    pub fn register(&mut self, integration: Arc<dyn Integration>) {
        let id = integration.id().to_string();
        self.enabled.insert(id.clone(), false); // disabled by default — user enables via UI
        self.integrations.insert(id, integration);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Integration>> {
        self.integrations.get(id).cloned()
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        self.enabled.insert(id.to_string(), enabled);
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        self.enabled.get(id).copied().unwrap_or(false)
    }

    /// BUG 4a (Discord reports of SpaceAcres/MystNodes cards "showing off
    /// until the user re-enables"): restore each integration's enabled flag
    /// from the persisted config at boot WITHOUT permanently disabling it
    /// just because `check_requirements()` fails on THIS launch. Previously a
    /// boot-time failure (a cold SSD probe, a disk briefly full) flipped the
    /// registry to disabled forever — the health loop's `enabled_fn` gate
    /// then never retried it again even once the machine started meeting the
    /// requirements, because nothing else ever re-enables it. `unavailable_reason`
    /// is already recomputed live on every `get_integrations` poll and tells
    /// the user why, independent of this flag, so disabling here only
    /// duplicated that signal while also breaking self-healing. Ids no longer
    /// registered (e.g. a removed integration) are skipped rather than
    /// inflating `enabled_count()`/`proportion()` with a ghost entry.
    pub fn restore_enabled_states(&mut self, configured: &HashMap<String, bool>) {
        for (id, &enabled) in configured {
            let Some(integration) = self.get(id) else {
                tracing::info!(
                    id = id.as_str(),
                    "Config references a removed integration — ignoring"
                );
                continue;
            };
            if enabled {
                if let Err(reason) = integration.check_requirements() {
                    tracing::info!(
                        id = id.as_str(),
                        reason = reason.as_str(),
                        "Integration does not currently meet its minimum requirements at boot — leaving it enabled so the health loop retries automatically"
                    );
                }
            }
            self.set_enabled(id, enabled);
        }
    }

    pub fn list(&self) -> Vec<Arc<dyn Integration>> {
        self.integrations.values().cloned().collect()
    }

    /// Fallback status derivation from registry metadata only.
    /// Prefer combining real health checks with registry state in commands.
    pub fn list_statuses(&self) -> Vec<IntegrationStatus> {
        self.integrations
            .values()
            .map(|i| {
                let id = i.id().to_string();
                let enabled = self.is_enabled(&id);
                IntegrationStatus {
                    id: id.clone(),
                    display_name: i.display_name().to_string(),
                    enabled,
                    health: if enabled {
                        HealthStatus::Starting
                    } else {
                        HealthStatus::Stopped
                    },
                    lifecycle: if enabled {
                        LifecycleState::Starting
                    } else {
                        LifecycleState::Disabled
                    },
                    version: i.installed_version(),
                    poc_contribution: if enabled {
                        1.0 / self.total_count() as f64
                    } else {
                        0.0
                    },
                    tier: tier_for(&id),
                    requires_docker: i.requires_docker(),
                    error: None,
                    unavailable_reason: i.check_requirements().err(),
                }
            })
            .collect()
    }

    pub fn enabled_count(&self) -> u32 {
        self.enabled.values().filter(|&&v| v).count() as u32
    }

    pub fn total_count(&self) -> u32 {
        self.integrations.len() as u32
    }

    /// Registered integrations this machine can actually run. This is the
    /// denominator for the PoC proportion: dividing by `total_count()` would
    /// dock every user for integrations their hardware rules out, so adding an
    /// integration nobody can run would silently cut everyone's rewards.
    pub fn available_count(&self) -> u32 {
        self.integrations
            .values()
            .filter(|i| i.check_requirements().is_ok())
            .count() as u32
    }

    /// Proportion of enabled integrations (0.0 to 1.0)
    pub fn proportion(&self) -> f64 {
        let total = self.total_count();
        if total == 0 {
            return 0.0;
        }
        self.enabled_count() as f64 / total as f64
    }
}

impl Default for IntegrationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// B4: what the user is actually told when a partner subprocess fails.
#[cfg(test)]
mod stderr_tail_tests {
    use super::*;

    /// Real `docker run` failure output. The cause is on the FIRST line and
    /// the last line is Docker's generic usage hint — `lines().last()` handed
    /// the user the hint and threw the cause away.
    const RUN_FAILURE: &str = "\
docker: Error response from daemon: driver failed programming external connectivity on endpoint fem-pawns: port is already allocated.
See 'docker run --help'.";

    /// A pull failure whose output ends in a whitespace-only progress line.
    const PULL_FAILURE: &str = "Error response from daemon: manifest unknown\n   \n";

    #[test]
    fn the_real_cause_survives_instead_of_dockers_usage_hint() {
        assert_eq!(
            RUN_FAILURE.lines().last(),
            Some("See 'docker run --help'."),
            "fixture must reproduce the bug: the old code showed only this line"
        );
        let shown = stderr_tail(RUN_FAILURE, 10);
        assert!(
            shown.contains("port is already allocated"),
            "shown was: {}",
            shown
        );
    }

    #[test]
    fn a_trailing_whitespace_line_no_longer_blanks_the_message() {
        assert_eq!(
            PULL_FAILURE.lines().last(),
            Some("   "),
            "fixture must reproduce the bug: the old code showed only whitespace"
        );
        assert_eq!(
            stderr_tail(PULL_FAILURE, 10),
            "Error response from daemon: manifest unknown"
        );
    }

    #[test]
    fn newlines_become_separators_within_the_requested_tail() {
        let stderr = "failed to solve\nprocess did not complete\nexit code: 1";
        assert_eq!(
            stderr_tail(stderr, 10),
            "failed to solve | process did not complete | exit code: 1"
        );
    }

    #[test]
    fn only_the_last_n_surviving_lines_are_kept() {
        let stderr = "one\ntwo\nthree\nfour";
        assert_eq!(stderr_tail(stderr, 2), "three | four");
    }

    #[test]
    fn blank_lines_do_not_consume_the_line_budget() {
        // The whole point: empties are dropped BEFORE the tail is taken, so a
        // padded log cannot push the cause out of the window.
        assert_eq!(stderr_tail("real cause\n\n\n\n", 2), "real cause");
    }

    #[test]
    fn entirely_empty_output_says_so_rather_than_showing_nothing() {
        // The pre-fix message ended in a bare colon; a caller must never be
        // able to render "Could not download the agent image: ".
        assert_eq!(stderr_tail("", 10), "no error output");
        assert_eq!(stderr_tail("\n\n   \n", 10), "no error output");
    }

    #[test]
    fn a_wall_of_docker_output_is_capped_on_a_character_boundary() {
        let huge = "é".repeat(5_000);
        let shown = stderr_tail(&huge, 10);
        assert!(shown.ends_with('…'));
        assert_eq!(shown.chars().count(), MAX_STDERR_CHARS + 1);
    }
}

#[cfg(test)]
mod tier_tests {
    use super::*;

    /// The five contracted partners. Kept literal: the reward copy on the
    /// Dashboard says "X / 5", and a silent addition here would change what
    /// every user is told they must run.
    const OFFICIAL: [&str; 5] = ["mysterium", "diiisco", "space_acres", "aem", "fryvpn"];
    const SDK: [&str; 5] = ["storj", "titan", "sentinel", "iagon", "pawns"];

    #[test]
    fn every_contracted_partner_is_official() {
        for id in OFFICIAL {
            assert_eq!(tier_for(id), IntegrationTier::Official, "id was: {}", id);
        }
    }

    #[test]
    fn every_community_build_is_sdk() {
        for id in SDK {
            assert_eq!(tier_for(id), IntegrationTier::Sdk, "id was: {}", id);
        }
    }

    #[test]
    fn an_unknown_id_is_never_promoted_to_official() {
        assert_eq!(tier_for("brand_new_partner"), IntegrationTier::Sdk);
        assert_eq!(tier_for(""), IntegrationTier::Sdk);
    }

    #[test]
    fn the_tier_serializes_lowercase_for_the_typescript_union() {
        // src/lib/integrationMeta.ts declares `'official' | 'sdk'`.
        assert_eq!(
            serde_json::to_string(&IntegrationTier::Official).unwrap(),
            "\"official\""
        );
        assert_eq!(
            serde_json::to_string(&IntegrationTier::Sdk).unwrap(),
            "\"sdk\""
        );
    }
}

/// BUG 4a: a boot-time requirements failure must not permanently disable an
/// integration the user configured as enabled.
#[cfg(test)]
mod restore_enabled_states_tests {
    use super::*;
    use async_trait::async_trait;

    struct AlwaysFailsRequirements;

    #[async_trait]
    impl Integration for AlwaysFailsRequirements {
        fn id(&self) -> &str {
            "always_fails"
        }
        fn display_name(&self) -> &str {
            "Always Fails"
        }
        async fn install(&self) -> Result<()> {
            Ok(())
        }
        async fn start(&self) -> Result<()> {
            Ok(())
        }
        async fn stop(&self) -> Result<()> {
            Ok(())
        }
        async fn health_check(&self) -> HealthStatus {
            HealthStatus::Unknown
        }
        async fn check_update(&self) -> Result<Option<String>> {
            Ok(None)
        }
        fn check_requirements(&self) -> Result<(), String> {
            Err("cold SSD probe: no SSD detected".to_string())
        }
    }

    struct AlwaysMeetsRequirements;

    #[async_trait]
    impl Integration for AlwaysMeetsRequirements {
        fn id(&self) -> &str {
            "always_ok"
        }
        fn display_name(&self) -> &str {
            "Always OK"
        }
        async fn install(&self) -> Result<()> {
            Ok(())
        }
        async fn start(&self) -> Result<()> {
            Ok(())
        }
        async fn stop(&self) -> Result<()> {
            Ok(())
        }
        async fn health_check(&self) -> HealthStatus {
            HealthStatus::Healthy
        }
        async fn check_update(&self) -> Result<Option<String>> {
            Ok(None)
        }
    }

    #[test]
    fn an_enabled_integration_stays_enabled_after_a_boot_time_requirements_failure() {
        let mut reg = IntegrationRegistry::new();
        reg.register(Arc::new(AlwaysFailsRequirements));

        let mut configured = HashMap::new();
        configured.insert("always_fails".to_string(), true);

        reg.restore_enabled_states(&configured);

        assert!(
            reg.is_enabled("always_fails"),
            "an integration the user enabled must stay enabled even if check_requirements() fails at boot, \
             so the health loop can retry it automatically once the machine meets requirements again"
        );
    }

    #[test]
    fn a_disabled_integration_stays_disabled_regardless_of_requirements() {
        let mut reg = IntegrationRegistry::new();
        reg.register(Arc::new(AlwaysMeetsRequirements));

        let mut configured = HashMap::new();
        configured.insert("always_ok".to_string(), false);

        reg.restore_enabled_states(&configured);

        assert!(!reg.is_enabled("always_ok"));
    }

    #[test]
    fn an_id_no_longer_registered_is_skipped_without_panicking() {
        let mut reg = IntegrationRegistry::new();
        let mut configured = HashMap::new();
        configured.insert("removed_integration".to_string(), true);

        reg.restore_enabled_states(&configured);

        assert!(!reg.is_enabled("removed_integration"));
    }
}

/// D-19 / B17 D1: the marker list is duplicated in three places by necessity —
/// here, in `src/lib/setupRequired.ts`, and as constants beside the code that
/// actually emits each reason. The TS side is guarded by
/// `setupRequiredParity.test.ts`; this guards the Rust side.
#[cfg(test)]
mod marker_parity_tests {
    use super::*;

    /// Each literal must equal the constant used by the code that emits it, so
    /// renaming a message cannot silently stop the supervisor recognising it.
    #[test]
    fn every_marker_matches_the_constant_its_emitter_uses() {
        assert!(AWAITING_MARKERS.contains(&fryvpn::FUNDING_MARKER));
        assert!(AWAITING_MARKERS.contains(&code_integrity::AWAITING_ADMIN_MARKER));
        assert!(AWAITING_MARKERS.contains(&port_conflict::PORT_HELD_MARKER));
    }

    /// The two B17 markers have no constant of their own, so pin them against
    /// the reasons Iagon and Sentinel really build.
    #[test]
    fn the_iagon_and_sentinel_markers_match_the_reasons_those_partners_emit() {
        let iagon = include_str!("iagon.rs");
        let sentinel = include_str!("sentinel.rs");

        assert!(
            iagon.contains("node token not provisioned"),
            "iagon.rs no longer emits the reason this marker matches"
        );
        assert!(
            sentinel.contains("node account not funded"),
            "sentinel.rs no longer emits the reason this marker matches"
        );
    }

    /// Every marker must actually be recognised, and nothing ordinary may be.
    #[test]
    fn each_marker_is_recognised_and_ordinary_failures_are_not() {
        for marker in AWAITING_MARKERS {
            assert!(
                awaits_user_action(&format!("Some partner: {marker} — do a thing")),
                "{marker} is in the list but not recognised"
            );
        }
        assert!(!awaits_user_action("Error detected in daemon logs"));
        assert!(!awaits_user_action("process is not running"));
        assert!(!awaits_user_action("Container exited"));
    }

    /// The list is a SET: a duplicate would make the TS parity comparison
    /// pass or fail for the wrong reason.
    #[test]
    fn the_markers_are_distinct() {
        let mut seen = AWAITING_MARKERS.to_vec();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(
            before,
            seen.len(),
            "duplicate marker in {AWAITING_MARKERS:?}"
        );
    }
}

/// B3 / G4: the guard whose ABSENCE let a release-blocking violation through.
///
/// titan's VC++ redistributable installer and the Docker Desktop installer both
/// raised `Start-Process -Verb RunAs` directly and referenced the elevation
/// gate nowhere — while `elevation_gate`'s own module doc listed both functions
/// among the five sites it covered. Both were reachable with NO user gesture
/// (the boot recovery pass calls `install()` for every enabled-but-not-installed
/// integration), so a UAC dialog appeared at app start for exactly the
/// populations that filed the missing-runtime and wiped-partner-files reports.
///
/// Converting the call sites is not enough on its own: nothing stopped the next
/// elevation site from being added the same way. This makes that structural.
#[cfg(test)]
mod b3_elevation_routing_tests {
    /// Every integration source that can raise a UAC prompt.
    const ELEVATION_SOURCES: [(&str, &str); 5] = [
        ("firewall.rs", include_str!("firewall.rs")),
        ("titan.rs", include_str!("titan.rs")),
        ("docker_manager.rs", include_str!("docker_manager.rs")),
        ("aem.rs", include_str!("aem.rs")),
        ("space_acres.rs", include_str!("space_acres.rs")),
    ];

    /// Strip line comments, so prose ABOUT an elevation cannot satisfy — or
    /// trip — this guard. Both matter here: these files deliberately describe
    /// the pattern they no longer use.
    fn code_only(src: &str) -> String {
        src.lines()
            .map(|l| l.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn no_integration_raises_a_uac_prompt_outside_the_elevation_gate() {
        // Assembled at runtime so this guard cannot match its own source text.
        let raises = format!("-Verb Run{}", "As");
        let gate = format!("elevation{}::run_elevated", "_gate");

        let mut checked = 0;
        for (name, src) in ELEVATION_SOURCES {
            let code = code_only(src);
            if !code.contains(&raises) {
                continue;
            }
            checked += 1;
            assert!(
                code.contains(&gate),
                "{name} raises a UAC prompt but never calls the elevation gate — FEM can \
                 elevate without a user gesture, which is exactly what B3 forbids"
            );
        }
        assert!(
            checked >= 3,
            "expected at least the three known UAC-raising integration sources \
             (firewall, titan, docker_manager), found {checked} — has a file been renamed?"
        );
    }

    /// A gate call is only meaningful if the trigger can be Automatic; a site
    /// that hard-codes UserClick has opted itself out of the policy.
    #[test]
    fn every_gated_site_can_refuse_an_automatic_trigger() {
        let raises = format!("-Verb Run{}", "As");
        let automatic = format!("ElevationTrigger::Auto{}", "matic");
        let user = format!("ElevationTrigger::User{}", "Click");

        for (name, src) in ELEVATION_SOURCES {
            let code = code_only(src);
            if !code.contains(&raises) {
                continue;
            }
            // Either the site takes a trigger from its caller, or it must name
            // Automatic somewhere. Hard-coding only UserClick is the failure.
            let takes_trigger = code.contains("trigger: crate::elevation_gate::ElevationTrigger")
                || code.contains("trigger,");
            assert!(
                takes_trigger || code.contains(&automatic),
                "{name} always elevates on its own authority; it must accept the caller's \
                 trigger so an automatic path can be refused"
            );
            let _ = &user;
        }
    }
}
