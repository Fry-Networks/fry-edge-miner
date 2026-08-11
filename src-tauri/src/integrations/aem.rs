use crate::supervisor::platform::BoundedOutput;
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
            // Squirrel dirs are app-X.Y.Z — compare version components
            // numerically; lexicographic order picks app-9.0 over app-10.0.
            .max_by_key(|e| {
                e.file_name()
                    .to_str()
                    .and_then(|n| n.strip_prefix("app-"))
                    .map(|v| {
                        v.split('.')
                            .map(|p| p.parse::<u32>().unwrap_or(0))
                            .collect::<Vec<u32>>()
                    })
                    .unwrap_or_default()
            })
            .map(|e| e.path().join("OlostepBrowser.exe"))
            .filter(|p| p.exists())
    }

    fn is_running() -> bool {
        #[cfg(target_os = "windows")]
        {
            crate::supervisor::platform::command("tasklist")
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
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

    /// Physical RAM, cached (0 = probe failed → memory check disabled).
    fn total_ram_bytes() -> u64 {
        static CACHE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
        *CACHE.get_or_init(|| {
            #[cfg(target_os = "windows")]
            {
                crate::supervisor::platform::command("powershell")
                    .args([
                        "-NoProfile",
                        "-Command",
                        "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory",
                    ])
                    .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
                    .ok()
                    .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().ok())
                    .unwrap_or(0)
            }
            #[cfg(not(target_os = "windows"))]
            {
                0
            }
        })
    }

    /// Summed WorkingSet + CPU-seconds of all OlostepBrowser processes.
    fn resource_sample() -> Option<crate::supervisor::resource_guard::Sample> {
        #[cfg(target_os = "windows")]
        {
            let out = crate::supervisor::platform::command("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    "$p = Get-Process OlostepBrowser -ErrorAction SilentlyContinue; \
                     if ($p) { $ws = ($p | Measure-Object WorkingSet64 -Sum).Sum; \
                     $cpu = ($p | Measure-Object CPU -Sum).Sum; Write-Output \"$ws|$cpu\" }",
                ])
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
                .ok()?;
            let text = String::from_utf8_lossy(&out.stdout);
            let mut parts = text.trim().split('|');
            let mem = parts.next()?.trim().parse::<u64>().ok()?;
            let cpu = parts.next()?.trim().replace(',', ".").parse::<f64>().unwrap_or(0.0);
            Some(crate::supervisor::resource_guard::Sample {
                mem_bytes: mem,
                cpu_seconds: cpu,
                at: std::time::Instant::now(),
            })
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    }

    /// Resource-cap check run from the 30s health tick. Some(reason) = the
    /// process must be restarted (supervisor handles it via Unhealthy).
    fn resource_breach() -> Option<String> {
        use crate::supervisor::resource_guard::{self, ResourceGuard, TripReason};
        static GUARD: std::sync::OnceLock<std::sync::Mutex<ResourceGuard>> =
            std::sync::OnceLock::new();
        let sample = Self::resource_sample()?;
        let mem_cap =
            (Self::total_ram_bytes() as f64 * resource_guard::MEM_CAP_FRACTION) as u64;
        let cores = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(1);
        let mut guard = GUARD
            .get_or_init(|| std::sync::Mutex::new(ResourceGuard::new()))
            .lock()
            .ok()?;
        match guard.evaluate(sample, mem_cap, resource_guard::CPU_CAP_FRACTION, cores)? {
            TripReason::Memory { used_bytes, cap_bytes } => Some(format!(
                "restarted: resource limit — OlostepBrowser held {:.1} GB of RAM (cap {:.1} GB)",
                used_bytes as f64 / 1e9,
                cap_bytes as f64 / 1e9
            )),
            TripReason::Cpu { fraction, cap_fraction } => Some(format!(
                "restarted: resource limit — OlostepBrowser sustained {:.0}% CPU (cap {:.0}%)",
                fraction * 100.0,
                cap_fraction * 100.0
            )),
        }
    }

    /// Last few lines of Squirrel's own install log — the only place the real
    /// install-failure reason (AV block, lock, disk) is recorded.
    fn squirrel_log_tail() -> Option<String> {
        let log = dirs::data_local_dir()?
            .join("SquirrelTemp")
            .join("SquirrelSetup.log");
        let contents = std::fs::read_to_string(log).ok()?;
        let tail: Vec<&str> = contents.lines().rev().take(3).collect();
        let mut joined = tail
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" | ");
        if joined.len() > 300 {
            joined = joined[joined.len() - 300..].to_string();
        }
        if joined.is_empty() { None } else { Some(joined) }
    }

    /// Force-clean every Olostep artifact so a reinstall starts from zero:
    /// kill the process (exit code ignored — "not running" is expected), then
    /// best-effort delete the install dir, Squirrel temp, and our installer temp.
    pub(crate) fn force_clean() {
        #[cfg(target_os = "windows")]
        {
            let _ = crate::supervisor::platform::command("taskkill")
                .args(["/IM", "OlostepBrowser.exe", "/F"])
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT);
        }
        // Rules are keyed to the install path being wiped below; deleting
        // them here (not on every disable) avoids a UAC prompt per toggle.
        super::firewall::delete_rules(super::firewall::OLOSTEP_RULE_NAME);
        let dirs_to_remove = [
            dirs::data_local_dir().map(|d| d.join("Olostep-Browser")),
            dirs::data_local_dir().map(|d| d.join("SquirrelTemp")),
            Some(partners_base_dir().join("aem")),
        ];
        for dir in dirs_to_remove.into_iter().flatten() {
            if dir.exists() {
                if let Err(e) = std::fs::remove_dir_all(&dir) {
                    warn!(path = ?dir, error = %e, "Force-clean: could not remove directory (continuing)");
                } else {
                    info!(path = ?dir, "Force-clean: removed");
                }
            }
        }
    }

    /// True when the Olostep config must be (re)staged: file missing, unreadable,
    /// unparseable, or the `mellowtel_opt_in_status` key absent — the signature of
    /// an Olostep Squirrel self-update wiping/resetting config.json. An explicit
    /// `false` value is a user opt-out made inside Olostep and is respected.
    fn config_needs_restage(contents: Option<&str>) -> bool {
        let Some(s) = contents else { return true };
        match serde_json::from_str::<serde_json::Value>(s) {
            Ok(v) => v.get("mellowtel_opt_in_status").and_then(|x| x.as_bool()).is_none(),
            Err(_) => true,
        }
    }

    /// Stage config.json with Mellowtel opt-in settings
    fn stage_config() -> Result<()> {
        let config_path = Self::olostep_config_path()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine config directory"))?;
        let existing = std::fs::read_to_string(&config_path).ok();
        if !Self::config_needs_restage(existing.as_deref()) {
            info!(path = ?config_path, "OlostepBrowser config already staged (opt-in key intact)");
            return Ok(());
        }
        if existing.is_some() {
            info!(path = ?config_path, "OlostepBrowser config lost its opt-in key (self-update wipe) — re-staging");
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
        "Olostep"
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
            .output_bounded(crate::supervisor::platform::LONG_TIMEOUT)?;
        if !output.status.success() {
            warn!(
                code = output.status.code(),
                "Installer exited with non-zero (Squirrel may still install in background)"
            );
        }
        // Squirrel installers run async — wait for binary to appear
        let mut appeared = false;
        for _ in 0..60 {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            if Self::olostep_binary().is_some() {
                appeared = true;
                break;
            }
        }
        if !appeared {
            let mut diag = String::new();
            if let Some(code) = output.status.code() {
                diag.push_str(&format!(" Installer exit code: {}.", code));
            }
            if let Some(tail) = Self::squirrel_log_tail() {
                diag.push_str(&format!(" Squirrel log: {}", tail));
            }
            anyhow::bail!(
                "OlostepBrowser installer ran but the app did not appear within 120 seconds — antivirus or a permission prompt may have blocked it.{} Use Reinstall on the Olostep card to retry from a clean slate.",
                diag
            );
        }
        Self::stage_config()?;
        if let Err(e) = std::fs::remove_file(&installer_path) {
            warn!(error = %e, "Could not remove OlostepBrowser installer (overwritten on next install)");
        }
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
        // Re-assert the staged config before launch — an Olostep self-update
        // may have wiped it, which would re-prompt the user for a permission
        // they already granted via the FEM toggle.
        if let Err(e) = Self::stage_config() {
            warn!(error = %e, "Could not re-stage OlostepBrowser config before start");
        }
        // Pre-create firewall rules for this exact binary path so Windows
        // never shows the firewall prompt (the path changes on every Olostep
        // Squirrel self-update, which re-triggered the prompt each time).
        // Non-fatal: a declined UAC just means Windows prompts as before.
        if let Err(e) = super::firewall::ensure_program_rules(
            super::firewall::OLOSTEP_RULE_NAME,
            &binary,
        ) {
            warn!(error = %e, "Olostep firewall rule setup failed — continuing");
        }
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
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT);
        }
        info!("Stopped OlostepBrowser");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        if Self::is_running() {
            // Resource caps (v0.4.8): field reports of OlostepBrowser
            // consuming all RAM / ~100% CPU. A breach reports Unhealthy so
            // the supervisor restart path (stop → taskkill → start) bounces
            // the process; the reason lands on the card.
            // block_in_place: the probe shells out to PowerShell (~<1s, but
            // it must not pin a tokio worker if PowerShell ever hangs).
            let breach = tokio::task::block_in_place(Self::resource_breach);
            if let Some(reason) = breach {
                warn!(reason = %reason, "OlostepBrowser resource cap breached — restarting");
                return HealthStatus::Unhealthy(reason);
            }
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
        // Self-heal: only reached for ENABLED integrations (poc/gates.rs filters
        // on is_enabled). If Olostep self-updated and wiped its config, restore
        // the previously granted opt-in before reading it, so poa doesn't zero
        // out and the user is never re-prompted.
        if Self::olostep_binary().is_some() {
            if let Err(e) = Self::stage_config() {
                warn!(error = %e, "Olostep config re-stage failed during PoC collection");
            }
        }
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

#[cfg(test)]
mod tests {
    use super::AemIntegration;

    #[test]
    fn missing_file_needs_restage() {
        assert!(AemIntegration::config_needs_restage(None));
    }

    #[test]
    fn wiped_config_without_opt_in_key_needs_restage() {
        // Squirrel self-update reset: file exists but the opt-in key is gone (F1).
        assert!(AemIntegration::config_needs_restage(Some("{}")));
        assert!(AemIntegration::config_needs_restage(Some(
            r#"{"terms-accepted":true,"auto-start-enabled":true}"#
        )));
    }

    #[test]
    fn intact_opt_in_does_not_restage() {
        assert!(!AemIntegration::config_needs_restage(Some(
            r#"{"mellowtel_opt_in_status":true}"#
        )));
    }

    #[test]
    fn explicit_opt_out_is_respected() {
        // User opted out inside Olostep — never overwrite that choice.
        assert!(!AemIntegration::config_needs_restage(Some(
            r#"{"mellowtel_opt_in_status":false}"#
        )));
    }

    #[test]
    fn corrupt_config_needs_restage() {
        assert!(AemIntegration::config_needs_restage(Some("not-json{")));
        assert!(AemIntegration::config_needs_restage(Some(
            r#"{"mellowtel_opt_in_status":"yes"}"#
        )));
    }
}
