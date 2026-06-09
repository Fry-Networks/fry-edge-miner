use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;

pub struct AemIntegration;

#[async_trait]
impl Integration for AemIntegration {
    fn id(&self) -> &str {
        "aem"
    }

    fn display_name(&self) -> &str {
        "AEM"
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

    async fn apply_update(&self, _version: &str) -> Result<()> {
        Ok(())
    }

    fn collect_poc_data(&self) -> PocGateData {
        PocGateData::default()
    }
}
