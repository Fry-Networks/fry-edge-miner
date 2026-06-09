use super::download::partners_base_dir;
use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use tracing::warn;

pub struct PresearchIntegration;

impl PresearchIntegration {
    fn partner_dir() -> PathBuf {
        partners_base_dir().join("presearch")
    }
}

#[async_trait]
impl Integration for PresearchIntegration {
    fn id(&self) -> &str {
        "presearch"
    }

    fn display_name(&self) -> &str {
        "Presearch"
    }

    async fn install(&self) -> Result<()> {
        let partner_dir = Self::partner_dir();
        std::fs::create_dir_all(&partner_dir)?;

        warn!(
            "DEFERRED: Presearch primarily uses Docker-based nodes. \
            No standalone Windows binary known. Docker support needed."
        );

        Ok(())
    }

    async fn start(&self) -> Result<()> {
        warn!("Presearch start: framework stub, Docker container needed");
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        warn!("Presearch stop: framework stub");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        // Framework stub - check if process would be running
        warn!("Presearch health check: framework stub");
        HealthStatus::Stopped
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
