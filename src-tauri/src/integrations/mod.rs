pub mod aem;
pub mod diiisco;
pub mod docker_manager;
pub mod download;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationStatus {
    pub id: String,
    pub display_name: String,
    pub enabled: bool,
    pub health: HealthStatus,
    pub lifecycle: LifecycleState,
    pub version: Option<String>,
    pub poc_contribution: f64,
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
