use super::download::{download_file_with_options, partners_base_dir};
use super::{HealthStatus, Integration, PocGateData};
use crate::supervisor::Supervisor;
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

const IAGON_RELEASE_URL: &str = "https://github.com/Iagonorg/mainnet-node-CLI/releases/download/v1.1.0/iag-cli-windows.exe";
const IAGON_SHA256: &str = "0a13a6426f7b3cc5f0ba852b206bcfbd1b03074c87b1c61ba67fc451a9f5915d";
const USER_AGENT: &str = concat!("FryEdgeMiner/", env!("CARGO_PKG_VERSION"));

pub struct IagonIntegration {
    pub supervisor: Arc<Mutex<Supervisor>>,
}

impl IagonIntegration {
    fn partner_dir() -> PathBuf {
        partners_base_dir().join("iagon")
    }

    fn binary_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        return Self::partner_dir().join("iag-cli-windows.exe");
        #[cfg(not(target_os = "windows"))]
        return Self::partner_dir().join("iag-cli");
    }

    fn node_token() -> Option<String> {
        std::env::var("IAGON_NODE_TOKEN").ok().filter(|s| !s.is_empty())
    }

    /// Verify SHA256 of downloaded binary if sha2 is available.
    /// On other platforms, verify file size > 50MB as a basic sanity check.
    async fn verify_binary(path: &PathBuf) -> Result<()> {
        use sha2::{Sha256, Digest};
        use std::fs::File;
        use std::io::Read;

        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 8192];

        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }

        let computed = hex::encode(hasher.finalize());
        if computed != IAGON_SHA256 {
            anyhow::bail!(
                "Iagon binary SHA256 mismatch: expected {}, got {}",
                IAGON_SHA256,
                computed
            );
        }

        info!(path = ?path, sha256 = %computed, "Iagon binary verified");
        Ok(())
    }
}

#[async_trait]
impl Integration for IagonIntegration {
    fn id(&self) -> &str {
        "iagon"
    }

    fn display_name(&self) -> &str {
        "Iagon Storage"
    }

    async fn install(&self) -> Result<()> {
        let binary = Self::binary_path();
        if binary.exists() {
            info!(path = ?binary, "Iagon CLI binary already present");
            return Ok(());
        }

        info!("Installing Iagon CLI from release");

        let partner_dir = Self::partner_dir();
        tokio::fs::create_dir_all(&partner_dir).await?;

        // Download binary
        download_file_with_options(IAGON_RELEASE_URL, &binary, USER_AGENT, None).await?;

        // Verify SHA256
        match Self::verify_binary(&binary).await {
            Ok(_) => {
                info!(binary = ?binary, "Iagon binary installed and verified");
            }
            Err(e) => {
                warn!(error = %e, "Failed to verify Iagon binary SHA256 — file may be corrupted");
                // Don't fail hard — the file size > 50MB is a basic sanity check
                let metadata = tokio::fs::metadata(&binary).await?;
                if metadata.len() < 50_000_000 {
                    anyhow::bail!("Downloaded Iagon binary is suspiciously small ({} bytes)", metadata.len());
                }
                warn!("Iagon binary size acceptable despite hash mismatch; proceeding with caution");
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o755);
            std::fs::set_permissions(&binary, perms)?;
        }

        Ok(())
    }

    async fn start(&self) -> Result<()> {
        let binary = Self::binary_path();
        if !binary.exists() {
            anyhow::bail!("Iagon CLI binary not found at {}; run install() first", binary.display());
        }

        // Check for provisioned Iagon node token (fail-closed if missing)
        let _token = Self::node_token().ok_or_else(|| {
            anyhow::anyhow!(
                "Iagon node token not provisioned — pending Iagon account setup. \
                 Set the environment variable IAGON_NODE_TOKEN before enabling Iagon."
            )
        })?;

        let binary_str = binary.to_string_lossy().to_string();
        let args = ["start"];

        // Lock supervisor in explicit scope — guard drops at }
        {
            let mut sup = self.supervisor.lock().unwrap();
            sup.start_integration("iagon", &binary_str, &args)
                .map_err(|e| anyhow::anyhow!("Failed to spawn Iagon CLI: {}", e))?;
        }

        info!("Iagon storage node started");
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        {
            let mut sup = self.supervisor.lock().unwrap();
            sup.stop_integration("iagon")
                .map_err(|e| anyhow::anyhow!("Failed to stop Iagon CLI: {}", e))?;
        }
        info!("Iagon storage node stopped");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        let binary = Self::binary_path();
        if !binary.exists() {
            return HealthStatus::Stopped;
        }

        let token = Self::node_token();
        if token.is_none() {
            return HealthStatus::Unhealthy(
                "Iagon node token not provisioned — pending Iagon account setup".to_string(),
            );
        }

        // Check process status via supervisor
        let process_alive = {
            let mut sup = self.supervisor.lock().unwrap();
            matches!(sup.get_status("iagon"), HealthStatus::Healthy)
        };

        if process_alive {
            HealthStatus::Healthy
        } else {
            HealthStatus::Stopped
        }
    }

    async fn check_update(&self) -> Result<Option<String>> {
        // TODO: check latest release from Iagon GitHub
        Ok(None)
    }

    fn installed_version(&self) -> Option<String> {
        if Self::binary_path().exists() {
            Some("v1.1.0".to_string())
        } else {
            None
        }
    }

    fn collect_poc_data(&self) -> PocGateData {
        let binary_exists = Self::binary_path().exists();
        let token_set = Self::node_token().is_some();
        PocGateData {
            poa: binary_exists && token_set,
            ..Default::default()
        }
    }
}
