pub mod aem;
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
    async fn start(&self) -> Result<()>;
    async fn stop(&self) -> Result<()>;
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
        assert!(shown.contains("port is already allocated"), "shown was: {}", shown);
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
