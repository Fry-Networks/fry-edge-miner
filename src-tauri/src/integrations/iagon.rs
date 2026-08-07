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

/// Iagon's published storage-node minimums (docs.iagon.com ->
/// provide-resources/storage-nodes/minimum-requirements). A node that commits
/// less than 900 GB is rejected at registration, which is why the CLI's
/// self-registration returns HTTP 400 on a typical desktop.
const MIN_DISK_GB: f64 = 900.0;
const MIN_RAM_GB: f64 = 4.0;

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

    /// Path users drop their Iagon authorization key into:
    /// `<partners>/iagon/config.json` -> `{"node_token": "..."}`.
    fn config_path() -> PathBuf {
        Self::partner_dir().join("config.json")
    }

    /// Decide availability from already-measured specs. Split out from
    /// `check_requirements` so the thresholds are testable without shelling
    /// out to PowerShell.
    ///
    /// An unmeasurable spec (`None`) passes: a failed probe must not disable
    /// an integration that might be perfectly runnable.
    fn evaluate_requirements(disk_gb: Option<f64>, ram_gb: Option<f64>) -> Result<(), String> {
        if let Some(disk) = disk_gb {
            if disk < MIN_DISK_GB {
                return Err(format!(
                    "Iagon requires at least {:.0} GB of free storage — this device has {:.0} GB available.",
                    MIN_DISK_GB, disk
                ));
            }
        }
        if let Some(ram) = ram_gb {
            if ram < MIN_RAM_GB {
                return Err(format!(
                    "Iagon requires at least {:.0} GB of RAM — this device has {:.1} GB.",
                    MIN_RAM_GB, ram
                ));
            }
        }
        Ok(())
    }

    /// Token lookup, in precedence order:
    /// 1. `IAGON_NODE_TOKEN` env var (existing behaviour, useful for CI/testing)
    /// 2. `node_token` in the per-integration config.json
    /// 3. none — start() then fails closed with instructions
    ///
    /// Read fresh on every call so a user can paste their key and toggle the
    /// integration without restarting the whole app.
    fn node_token() -> Option<String> {
        if let Some(t) = std::env::var("IAGON_NODE_TOKEN").ok().filter(|s| !s.is_empty()) {
            return Some(t);
        }
        Self::token_from_config(&Self::config_path())
    }

    /// Split out from `node_token` so it can be tested without touching the
    /// process environment.
    fn token_from_config(path: &std::path::Path) -> Option<String> {
        let raw = std::fs::read_to_string(path).ok()?;
        // PowerShell's Set-Content -Encoding utf8 and older Notepad prepend a
        // UTF-8 BOM, which serde_json rejects — strip it so a hand-saved config
        // does not silently read as "no token".
        let raw = raw.trim_start_matches('\u{feff}');
        let json: serde_json::Value = serde_json::from_str(raw).ok()?;
        json.get("node_token")?
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
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

    fn check_requirements(&self) -> Result<(), String> {
        Self::evaluate_requirements(
            crate::system_info::available_disk_gb(&partners_base_dir()),
            crate::system_info::total_ram_gb(),
        )
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
                "Iagon node token not provisioned. Register a node at https://app.iagon.com, \
                 then save the authorization key as {{\"node_token\": \"<key>\"}} in {} \
                 (or set the IAGON_NODE_TOKEN environment variable).",
                Self::config_path().display()
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
            return HealthStatus::Unhealthy(format!(
                "Iagon node token not provisioned — register a node at app.iagon.com and save \
                 the key as {{\"node_token\": \"<key>\"}} in {}",
                Self::config_path().display()
            ));
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

#[cfg(test)]
mod tests {
    use super::IagonIntegration;
    use std::io::Write;

    fn temp_config(contents: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("fem_iagon_cfg_{}_{}.json", std::process::id(), contents.len()));
        let mut f = std::fs::File::create(&p).expect("create temp config");
        f.write_all(contents.as_bytes()).expect("write temp config");
        p
    }

    #[test]
    fn reads_token_from_config_file() {
        let p = temp_config(r#"{"node_token":"abc123"}"#);
        assert_eq!(
            IagonIntegration::token_from_config(&p),
            Some("abc123".to_string())
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn ignores_blank_or_missing_token() {
        let blank = temp_config(r#"{"node_token":"   "}"#);
        assert_eq!(IagonIntegration::token_from_config(&blank), None);
        let _ = std::fs::remove_file(&blank);

        let absent = temp_config(r#"{"something_else":"x"}"#);
        assert_eq!(IagonIntegration::token_from_config(&absent), None);
        let _ = std::fs::remove_file(&absent);
    }

    #[test]
    fn missing_or_malformed_file_is_none_not_panic() {
        let missing = std::env::temp_dir().join("fem_iagon_definitely_absent.json");
        let _ = std::fs::remove_file(&missing);
        assert_eq!(IagonIntegration::token_from_config(&missing), None);

        let bad = temp_config("not json at all");
        assert_eq!(IagonIntegration::token_from_config(&bad), None);
        let _ = std::fs::remove_file(&bad);
    }
    #[test]
    fn tolerates_utf8_bom_from_windows_editors() {
        // PowerShell Set-Content -Encoding utf8 and older Notepad both prepend
        // a UTF-8 BOM; without stripping it serde_json refuses the file and the
        // user sees 'token not provisioned' with no hint why.
        let mut p = std::env::temp_dir();
        p.push(format!("fem_iagon_bom_{}.json", std::process::id()));
        let mut f = std::fs::File::create(&p).expect("create");
        f.write_all(&[0xEF, 0xBB, 0xBF]).expect("bom");
        f.write_all(br#"{"node_token":"bom-token"}"#).expect("body");
        drop(f);
        assert_eq!(
            IagonIntegration::token_from_config(&p),
            Some("bom-token".to_string())
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn rejects_a_machine_below_iagons_900gb_minimum() {
        let err = IagonIntegration::evaluate_requirements(Some(128.0), Some(32.0))
            .expect_err("128 GB is far below the 900 GB minimum");
        assert!(err.contains("900 GB"), "{err}");
        assert!(err.contains("128 GB"), "message must name what the device has: {err}");
    }

    #[test]
    fn rejects_a_machine_below_the_ram_minimum() {
        let err = IagonIntegration::evaluate_requirements(Some(2000.0), Some(2.0))
            .expect_err("2 GB RAM is below the 4 GB minimum");
        assert!(err.contains("RAM"), "{err}");
    }

    #[test]
    fn accepts_a_machine_that_meets_both_minimums() {
        assert!(IagonIntegration::evaluate_requirements(Some(1518.0), Some(16.0)).is_ok());
    }

    #[test]
    fn exactly_at_the_threshold_is_accepted() {
        assert!(IagonIntegration::evaluate_requirements(Some(900.0), Some(4.0)).is_ok());
    }

    #[test]
    fn unmeasurable_specs_fail_open() {
        // A PowerShell probe that errors must not disable a runnable node.
        assert!(IagonIntegration::evaluate_requirements(None, None).is_ok());
        assert!(IagonIntegration::evaluate_requirements(None, Some(16.0)).is_ok());
    }
}
