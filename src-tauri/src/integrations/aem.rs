use super::download::{download_file, partners_base_dir};
use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use tracing::{info, warn};

const OLOSTEP_DOWNLOAD_URL: &str =
    "https://olostepbrowser.s3.us-east-1.amazonaws.com/setup.exe";

pub struct AemIntegration;

impl AemIntegration {
    /// Find OlostepBrowser.exe under %LOCALAPPDATA%\Olostep-Browser\app-*\
    fn olostep_binary() -> Option<PathBuf> {
        let local_app = dirs::data_local_dir()?;
        let install_dir = local_app.join("Olostep-Browser");
        if !install_dir.exists() {
            return None;
        }
        std::fs::read_dir(&install_dir)
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.starts_with("app-"))
                    .unwrap_or(false)
            })
            .max_by_key(|e| e.file_name())
            .map(|e| e.path().join("OlostepBrowser.exe"))
            .filter(|p| p.exists())
    }

    fn is_running() -> bool {
        #[cfg(target_os = "windows")]
        {
            crate::supervisor::platform::command("tasklist")
                .output()
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .to_lowercase()
                        .contains("olostepbrowser")
                })
                .unwrap_or(false)
        }
        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    }

    fn olostep_config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("Olostep-Browser").join("config.json"))
    }

    /// Stage config.json with Mellowtel opt-in settings
    fn stage_config() -> Result<()> {
        let config_path = Self::olostep_config_path()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine config directory"))?;
        if config_path.exists() {
            info!(path = ?config_path, "OlostepBrowser config already exists");
            return Ok(());
        }
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let identifier = format!(
            "mllwtl_olostepbrowser_{}",
            hex::encode(&rand::random::<[u8; 6]>())
        );
        let config = serde_json::json!({
            "mllwtl_identifier": identifier,
            "terms-accepted": true,
            "mellowtel_opt_in_status": true,
            "auto-start-enabled": true,
            "timestamp_m": chrono::Utc::now().timestamp_millis(),
            "count_m": 0
        });
        std::fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;
        info!(path = ?config_path, "Staged OlostepBrowser config");
        Ok(())
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
        if Self::olostep_binary().is_some() {
            info!("OlostepBrowser already installed");
            Self::stage_config()?;
            return Ok(());
        }

        info!("Downloading OlostepBrowser installer");
        let temp_dir = partners_base_dir().join("aem");
        std::fs::create_dir_all(&temp_dir)?;
        let installer_path = temp_dir.join("olostep-setup.exe");
        download_file(OLOSTEP_DOWNLOAD_URL, &installer_path).await?;

        info!("Running OlostepBrowser installer (Squirrel silent install)");
        let output = crate::supervisor::platform::command(&installer_path)
            .args(["--silent"])
            .output()?;
        if !output.status.success() {
            warn!(
                code = output.status.code(),
                "Installer exited with non-zero (Squirrel may still install in background)"
            );
        }
        // Squirrel installers run async — wait for binary to appear
        for _ in 0..30 {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            if Self::olostep_binary().is_some() {
                break;
            }
        }
        Self::stage_config()?;
        std::fs::remove_file(&installer_path).ok();
        info!("OlostepBrowser installation complete");
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        if Self::is_running() {
            info!("OlostepBrowser already running");
            return Ok(());
        }
        let binary = Self::olostep_binary()
            .ok_or_else(|| anyhow::anyhow!("OlostepBrowser not installed"))?;
        info!(binary = ?binary, "Starting OlostepBrowser");
        crate::supervisor::platform::command(&binary).spawn()?;
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            let _ = crate::supervisor::platform::command("taskkill")
                .args(["/IM", "OlostepBrowser.exe", "/F"])
                .output();
        }
        info!("Stopped OlostepBrowser");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        if Self::is_running() {
            HealthStatus::Healthy
        } else if Self::olostep_binary().is_some() {
            HealthStatus::Stopped
        } else {
            HealthStatus::Unknown
        }
    }

    async fn check_update(&self) -> Result<Option<String>> {
        Ok(None) // Squirrel handles auto-updates
    }

    async fn apply_update(&self, _version: &str) -> Result<()> {
        Ok(()) // Squirrel handles auto-updates
    }

    fn installed_version(&self) -> Option<String> {
        if Self::olostep_binary().is_some() {
            Some("installed".into())
        } else {
            None
        }
    }

    fn collect_poc_data(&self) -> PocGateData {
        let running = Self::is_running();
        let config_ok = Self::olostep_config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("mellowtel_opt_in_status")?.as_bool())
            .unwrap_or(false);
        PocGateData {
            poa: running && config_ok,
            ..Default::default()
        }
    }
}
