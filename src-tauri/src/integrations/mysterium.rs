use super::{HealthStatus, Integration};
use anyhow::Result;
use async_trait::async_trait;

pub struct MysteriumIntegration;

#[async_trait]
impl Integration for MysteriumIntegration {
    fn id(&self) -> &str {
        "mysterium"
    }

    fn display_name(&self) -> &str {
        "Mysterium (VPN/Bandwidth)"
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
