pub mod aem;
pub mod diiisco;
pub mod download;
pub mod mysterium;
pub mod presearch;
pub mod space_acres;

use std::collections::HashMap;

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
}

// --- Registry ---

pub struct IntegrationRegistry {
    integrations: HashMap<String, Box<dyn Integration>>,
    enabled: HashMap<String, bool>,
}

impl IntegrationRegistry {
    pub fn new() -> Self {
        Self {
            integrations: HashMap::new(),
            enabled: HashMap::new(),
        }
    }

    pub fn register(&mut self, integration: Box<dyn Integration>) {
        let id = integration.id().to_string();
        self.enabled.insert(id.clone(), true); // enabled by default
        self.integrations.insert(id, integration);
    }

    pub fn get(&self, id: &str) -> Option<&dyn Integration> {
        self.integrations.get(id).map(|b| b.as_ref())
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        self.enabled.insert(id.to_string(), enabled);
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        self.enabled.get(id).copied().unwrap_or(false)
    }

    pub fn list(&self) -> Vec<&dyn Integration> {
        self.integrations.values().map(|b| b.as_ref()).collect()
    }

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
                        HealthStatus::Healthy
                    } else {
                        HealthStatus::Stopped
                    },
                    lifecycle: if enabled {
                        LifecycleState::Running
                    } else {
                        LifecycleState::Disabled
                    },
                    version: None,
                    poc_contribution: if enabled {
                        1.0 / self.total_count() as f64
                    } else {
                        0.0
                    },
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
