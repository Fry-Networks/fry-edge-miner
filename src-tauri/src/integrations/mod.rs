pub mod mysterium;
pub mod presearch;
pub mod diiisco;
pub mod space_acres;
pub mod aem;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum HealthStatus {
    Healthy,
    Unhealthy(String),
    Stopped,
    Installing,
    Starting,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationStatus {
    pub id: String,
    pub display_name: String,
    pub enabled: bool,
    pub health: HealthStatus,
    pub version: Option<String>,
}

#[async_trait]
pub trait Integration: Send + Sync {
    fn id(&self) -> &str;
    fn display_name(&self) -> &str;
    async fn install(&self) -> Result<()>;
    async fn start(&self) -> Result<()>;
    async fn stop(&self) -> Result<()>;
    async fn health_check(&self) -> HealthStatus;
    async fn check_update(&self) -> Result<Option<String>>;
}
