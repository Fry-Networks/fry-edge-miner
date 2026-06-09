use super::download::partners_base_dir;
use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use tracing::warn;

pub struct AemIntegration;

impl AemIntegration {
    fn partner_dir() -> PathBuf {
        partners_base_dir().join("aem")
    }
}

#[async_trait]
impl Integration for AemIntegration {
    fn id(&self) -> &str {
        "aem"
    }

    fn display_name(&self) -> &str {
        "AEM"
    }

    async fn install(&self) -> Result<()> {
        let partner_dir = Self::partner_dir();
        std::fs::create_dir_all(&partner_dir)?;

        warn!(
            "DEFERRED: AI Edge Miner binary TBD. Framework ready for sensor integration."
        );

        Ok(())
    }

    async fn start(&self) -> Result<()> {
        warn!("DEFERRED: AEM start framework stub, binary TBD");
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        warn!("DEFERRED: AEM stop framework stub");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        // Sensor framework always ready
        HealthStatus::Healthy
    }

    async fn check_update(&self) -> Result<Option<String>> {
        Ok(None)
    }

    async fn apply_update(&self, _version: &str) -> Result<()> {
        warn!("DEFERRED: AEM update framework stub");
        Ok(())
    }

    fn collect_poc_data(&self) -> PocGateData {
        PocGateData::default()
    }
}
