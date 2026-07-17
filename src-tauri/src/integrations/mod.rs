pub mod aem;
pub mod diiisco;
pub mod docker_manager;
pub mod download;
pub mod fryvpn;
pub mod mysterium;
pub mod presearch;
pub mod space_acres;

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
