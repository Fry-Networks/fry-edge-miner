use super::{HealthStatus, Integration};
use anyhow::Result;
use async_trait::async_trait;

pub struct AemIntegration;

#[async_trait]
impl Integration for AemIntegration {
    fn id(&self) -> &str {
        "aem"
    }

    fn display_name(&self) -> &str {
        "AEM (AI Edge)"
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
