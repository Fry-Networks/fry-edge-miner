use super::download::partners_base_dir;
use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use tracing::warn;

pub struct DiiiscoIntegration;

impl DiiiscoIntegration {
    fn partner_dir() -> PathBuf {
        partners_base_dir().join("diiisco")
    }
}

#[async_trait]
impl Integration for DiiiscoIntegration {
    fn id(&self) -> &str {
        "diiisco"
    }

    fn display_name(&self) -> &str {
        "Diiisco"
    }

    async fn install(&self) -> Result<()> {
        let partner_dir = Self::partner_dir();
        std::fs::create_dir_all(&partner_dir)?;

        warn!(
            "DEFERRED: Diiisco binary/download URL unknown. \
            Framework ready, awaiting binary specification."
        );

        Ok(())
    }

    async fn start(&self) -> Result<()> {
        warn!("DEFERRED: Diiisco start framework stub, binary TBD");
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        warn!("DEFERRED: Diiisco stop framework stub");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        warn!("DEFERRED: Diiisco health check framework stub");
        HealthStatus::Unknown
    }

    async fn check_update(&self) -> Result<Option<String>> {
        Ok(None)
    }

    async fn apply_update(&self, _version: &str) -> Result<()> {
        warn!("DEFERRED: Diiisco update framework stub");
        Ok(())
    }

    fn collect_poc_data(&self) -> PocGateData {
        PocGateData::default()
    }
}
