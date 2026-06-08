use super::{HealthStatus, Integration};
use anyhow::Result;
use async_trait::async_trait;

pub struct SpaceAcresIntegration;

#[async_trait]
impl Integration for SpaceAcresIntegration {
    fn id(&self) -> &str {
        "space_acres"
    }

    fn display_name(&self) -> &str {
        "Space Acres (Storage)"
    }

    async fn install(&self) -> Result<()> {
        todo!("Phase 4")
    }

    async fn start(&self) -> Result<()> {
        todo!("Phase 4")
    }

    async fn stop(&self) -> Result<()> {
        todo!("Phase 4")
    }

    async fn health_check(&self) -> HealthStatus {
        HealthStatus::Stopped
    }

    async fn check_update(&self) -> Result<Option<String>> {
        Ok(None)
    }
}
