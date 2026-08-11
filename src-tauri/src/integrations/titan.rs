use crate::supervisor::platform::BoundedOutput;
use super::download::{download_file_with_options, partners_base_dir};
use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};
use crate::supervisor::Supervisor;

const DOWNLOAD_URL: &str = "https://github.com/Titannet-dao/titan-node/releases/download/v0.1.20/titan-edge_v0.1.20_246b9dd_widnows_amd64.tar.gz";
const EXPECTED_SHA256: &str = "6f37eea5cfcd6f799cd629d6e02a5636fb5c92995f73f0791ec0ff473afb558c";
const USER_AGENT: &str = concat!("FryEdgeMiner/", env!("CARGO_PKG_VERSION"));
const DAEMON_URL: &str = "https://cassini-locator.titannet.io:5000/rpc/v0";

pub struct TitanIntegration {
    pub supervisor: Arc<Mutex<Supervisor>>,
    pub log_dir: PathBuf,
}

impl TitanIntegration {
    fn partner_dir() -> PathBuf {
        partners_base_dir().join("titan")
    }

    fn binary_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        return Self::partner_dir().join("titan-edge.exe");
        #[cfg(not(target_os = "windows"))]
        return Self::partner_dir().join("titan-edge");
    }

    fn dll_path() -> PathBuf {
        Self::partner_dir().join("goworkerd.dll")
    }

    fn compute_sha256(path: &PathBuf) -> Result<String> {
        use sha2::{Sha256, Digest};
        use std::io::Read;

        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }
}

#[async_trait]
impl Integration for TitanIntegration {
    fn id(&self) -> &str {
        "titan"
    }

    fn display_name(&self) -> &str {
        "Titan Network"
    }

    async fn install(&self) -> Result<()> {
        let binary = Self::binary_path();
        if binary.exists() {
            info!(path = ?binary, "titan-edge binary already present");
            return Ok(());
        }

        info!("Installing Titan Network from GitHub release");

        let partner_dir = Self::partner_dir();
        tokio::fs::create_dir_all(&partner_dir).await?;

        let archive_path = partner_dir.join("titan-edge.tar.gz");

        // Download the archive
        download_file_with_options(DOWNLOAD_URL, &archive_path, USER_AGENT, None).await?;
        info!(archive = ?archive_path, "Downloaded Titan release archive");

        // Verify SHA256
        let computed_sha256 = Self::compute_sha256(&archive_path)?;
        if computed_sha256 != EXPECTED_SHA256 {
            let _ = tokio::fs::remove_file(&archive_path).await;
            anyhow::bail!(
                "SHA256 mismatch for titan-edge.tar.gz: expected {}, got {}",
                EXPECTED_SHA256,
                computed_sha256
            );
        }
        info!("SHA256 verification passed");

        // Extract using tar command (Windows 10+ includes bsdtar)
        let output = crate::supervisor::platform::command("tar")
            .args(["-xzf", &archive_path.to_string_lossy(), "-C", &partner_dir.to_string_lossy()])
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(stderr = %stderr, "tar extraction failed");
            anyhow::bail!("Failed to extract titan-edge.tar.gz: {}", stderr);
        }
        info!("Extracted archive");

        // The archive contains titan-edge_v0.1.20_246b9dd_widnows_amd64/ with titan-edge.exe and goworkerd.dll
        // Move both files to the parent directory
        let extracted_dir = partner_dir.join("titan-edge_v0.1.20_246b9dd_widnows_amd64");
        if extracted_dir.exists() {
            let exe_in_subdir = extracted_dir.join("titan-edge.exe");
            let dll_in_subdir = extracted_dir.join("goworkerd.dll");

            if exe_in_subdir.exists() {
                tokio::fs::rename(&exe_in_subdir, &binary).await?;
                info!(path = ?binary, "Moved titan-edge.exe to partner dir");
            }

            if dll_in_subdir.exists() {
                tokio::fs::rename(&dll_in_subdir, &Self::dll_path()).await?;
                info!(path = ?Self::dll_path(), "Moved goworkerd.dll to partner dir");
            }

            // Clean up the extracted subdirectory
            let _ = tokio::fs::remove_dir(&extracted_dir).await;
        }

        // Clean up archive
        let _ = tokio::fs::remove_file(&archive_path).await;

        info!(binary = ?binary, "Titan Network installed successfully");
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        let binary = Self::binary_path();
        if !binary.exists() {
            anyhow::bail!("titan-edge binary not found at {}", binary.display());
        }

        let binary_str = binary.to_string_lossy().to_string();
        let args = [
            "daemon",
            "start",
            "--init",
            &format!("--url={}", DAEMON_URL),
        ];

        {
            let mut sup = self.supervisor.lock().unwrap();
            sup.start_integration("titan", &binary_str, &args)
                .map_err(|e| anyhow::anyhow!("Failed to spawn titan-edge daemon: {}", e))?;
        }

        info!("Titan Network daemon started");
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        {
            let mut sup = self.supervisor.lock().unwrap();
            sup.stop_integration("titan")
                .map_err(|e| anyhow::anyhow!("Failed to stop titan-edge daemon: {}", e))?;
        }
        info!("Titan Network daemon stopped");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        // Check process alive
        let process_alive = {
            let mut sup = self.supervisor.lock().unwrap();
            matches!(sup.get_status("titan"), HealthStatus::Healthy)
        };

        if !process_alive {
            return HealthStatus::Stopped;
        }

        // Read both log files (stdout and stderr)
        let stdout_path = self.log_dir.join("titan").join("titan_stdout.log");
        let stderr_path = self.log_dir.join("titan").join("titan_stderr.log");

        let stdout_content = tokio::fs::read_to_string(&stdout_path).await.unwrap_or_default();
        let stderr_content = tokio::fs::read_to_string(&stderr_path).await.unwrap_or_default();

        // Combine recent tail (last 50 lines of each)
        let recent_lines: Vec<String> = stdout_content
            .lines()
            .chain(stderr_content.lines())
            .rev()
            .take(50)
            .map(|l| l.to_string())
            .collect();

        // Check for error markers
        let has_errors = recent_lines
            .iter()
            .any(|l| l.contains("error") || l.contains("ERROR") || l.contains("fatal"));

        if has_errors {
            return HealthStatus::Unhealthy("Error detected in Titan daemon logs".to_string());
        }

        // Check for daemon startup markers (conservative: process alive + no errors = Starting, look for running marker)
        let is_running = recent_lines
            .iter()
            .any(|l| l.contains("Daemon") || l.contains("edge") || l.contains("listening"));

        if is_running {
            HealthStatus::Healthy
        } else {
            // Process up but daemon not yet fully started
            HealthStatus::Starting
        }
    }

    async fn check_update(&self) -> Result<Option<String>> {
        // TODO: partner binary version check via /versions/titan
        Ok(None)
    }

    fn installed_version(&self) -> Option<String> {
        if Self::binary_path().exists() {
            Some("v0.1.20".into())
        } else {
            None
        }
    }

    fn collect_poc_data(&self) -> PocGateData {
        // Sync — supervisor status only
        let status = {
            let mut sup = self.supervisor.lock().unwrap();
            sup.get_status("titan")
        };
        PocGateData {
            poa: matches!(status, HealthStatus::Healthy),
            ..Default::default()
        }
    }
}
