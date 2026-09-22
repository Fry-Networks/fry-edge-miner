use super::download::{download_file, partners_base_dir};
use super::{tracked_child_probe, HealthStatus, Integration, PocGateData};
use crate::supervisor::platform::BoundedOutput;
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::{info, warn};

const OLOSTEP_DOWNLOAD_URL: &str = "https://olostepbrowser.s3.us-east-1.amazonaws.com/setup.exe";

/// BUG 10 (Discord: ~380 ghost tray icons). OlostepBrowser was spawned via a
/// bare `Command::spawn()` with the returned `Child` immediately discarded —
/// FEM had no way to tell its own spawned instance apart from one Windows
/// itself autostarted at login (the staged config sets
/// `auto-start-enabled: true`) or one left over from a previous session.
/// Every liveness check, start-guard, and stop went through an untargeted
/// `tasklist`/`taskkill /IM` image-name scan that ALSO failed OPEN on a
/// probe timeout (`.unwrap_or(false)`) — exactly the combination that lets
/// a slow/failed probe read as "not running" and spawn a duplicate on top
/// of a browser that never fully released its tray icon.
#[derive(Default)]
pub struct AemIntegration {
    child: Mutex<Option<std::process::Child>>,
}

impl AemIntegration {
    /// The real start. `trigger` decides whether the firewall rule may raise a
    /// UAC prompt: only a user gesture ever may (B3).
    async fn start_inner(&self, trigger: crate::elevation_gate::ElevationTrigger) -> Result<()> {
        if self.is_running() {
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
            "aem",
            trigger,
        ) {
            warn!(error = %e, "Olostep firewall rule setup failed — continuing");
        }
        info!(binary = ?binary, "Starting OlostepBrowser");
        let child = crate::supervisor::platform::command(&binary).spawn()?;
        // B4 (D-03): every partner FEM spawns joins the kill-on-close job,
        // Olostep included, so an abnormal FEM exit cannot leave a browser
        // running with no owner. FEM's normal quit stops partners gracefully
        // first (main.rs's ExitRequested handler), so the job's abrupt kill
        // only happens when FEM itself died abnormally — the orphan case.
        // B20: the same job carries BELOW_NORMAL, which reaches Olostep's
        // Chromium renderer/GPU/utility children as well as the parent.
        crate::supervisor::platform::adopt_into_partner_job(&child);
        // BUG 10: track the child so is_running()/stop() can target it
        // directly instead of only an untargeted image-name scan.
        if let Ok(mut guard) = self.child.lock() {
            *guard = Some(child);
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        Ok(())
    }

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

    /// Image-name tasklist probe — fallback for an adopted/untracked
    /// instance (Windows autostarted it, or a previous FEM session spawned
    /// it and this process restarted). BUG 10: previously failed OPEN
    /// (`.unwrap_or(false)`) — a tasklist timeout or error read as "not
    /// running" and could spawn a duplicate on top of a browser that was
    /// actually alive, which is exactly how ~380 ghost tray icons
    /// accumulated. Fails CLOSED now: assume running when the probe itself
    /// could not complete.
    fn image_name_probe() -> bool {
        #[cfg(target_os = "windows")]
        {
            crate::supervisor::platform::command("tasklist")
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .to_lowercase()
                        .contains("olostepbrowser")
                })
                .unwrap_or(true)
        }
        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    }

    /// Whether OlostepBrowser is running. Prefers the tracked child (fast,
    /// exact, no shell-out) and falls back to the image-name probe for an
    /// adopted instance FEM did not spawn itself.
    fn is_running(&self) -> bool {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(running) = tracked_child_probe(&mut guard) {
                return running;
            }
        }
        Self::image_name_probe()
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
                    .and_then(|o| {
                        String::from_utf8_lossy(&o.stdout)
                            .trim()
                            .parse::<u64>()
                            .ok()
                    })
                    .unwrap_or(0)
            }
            #[cfg(not(target_os = "windows"))]
            {
                0
            }
        })
    }

    /// Summed WorkingSet + CPU-seconds of all OlostepBrowser processes.
    ///
    /// B20: the same tick also drops every OlostepBrowser process to
    /// BELOW_NORMAL. The job object covers what FEM SPAWNS, but FEM's own
    /// staged config (`"auto-start-enabled": true`) makes Windows start Olostep
    /// at logon, so FEM usually ADOPTS an instance it never spawned — and the
    /// 40% CPU the user reported lives in the Chromium renderer/GPU/utility
    /// children, which FEM holds no handle to either. Folded into the EXISTING
    /// per-tick PowerShell so the tick starts no extra process, and unelevated,
    /// which is why B20's "no popups, no elevation" soak still holds:
    /// lowering priority on same-user processes needs no admin.
    fn resource_sample() -> Option<crate::supervisor::resource_guard::Sample> {
        #[cfg(target_os = "windows")]
        {
            let script = format!(
                "{}; $p = Get-Process OlostepBrowser -ErrorAction SilentlyContinue; \
                 if ($p) {{ $ws = ($p | Measure-Object WorkingSet64 -Sum).Sum; \
                 $cpu = ($p | Measure-Object CPU -Sum).Sum; Write-Output \"$ws|$cpu\" }}",
                crate::supervisor::platform::below_normal_script("OlostepBrowser")
            );
            let out = crate::supervisor::platform::command("powershell")
                .args(["-NoProfile", "-Command", &script])
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
                .ok()?;
            let text = String::from_utf8_lossy(&out.stdout);
            let mut parts = text.trim().split('|');
            let mem = parts.next()?.trim().parse::<u64>().ok()?;
            let cpu = parts
                .next()?
                .trim()
                .replace(',', ".")
                .parse::<f64>()
                .unwrap_or(0.0);
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
        let mem_cap = (Self::total_ram_bytes() as f64 * resource_guard::MEM_CAP_FRACTION) as u64;
        let cores = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(1);
        let mut guard = GUARD
            .get_or_init(|| std::sync::Mutex::new(ResourceGuard::new()))
            .lock()
            .ok()?;
        match guard.evaluate(sample, mem_cap, resource_guard::CPU_CAP_FRACTION, cores)? {
            TripReason::Memory {
                used_bytes,
                cap_bytes,
            } => Some(format!(
                "restarted: resource limit — OlostepBrowser held {:.1} GB of RAM (cap {:.1} GB)",
                used_bytes as f64 / 1e9,
                cap_bytes as f64 / 1e9
            )),
            TripReason::Cpu {
                fraction,
                cap_fraction,
            } => Some(format!(
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
        let mut joined = tail.into_iter().rev().collect::<Vec<_>>().join(" | ");
        if joined.len() > 300 {
            joined = joined[joined.len() - 300..].to_string();
        }
        if joined.is_empty() {
            None
        } else {
            Some(joined)
        }
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
        // Force-clean is reached only from the user's own reinstall click.
        super::firewall::delete_rules(
            super::firewall::OLOSTEP_RULE_NAME,
            "aem",
            crate::elevation_gate::ElevationTrigger::UserClick,
        );
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
            Ok(v) => v
                .get("mellowtel_opt_in_status")
                .and_then(|x| x.as_bool())
                .is_none(),
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
            hex::encode(rand::random::<[u8; 6]>())
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
        self.start_inner(crate::elevation_gate::ElevationTrigger::Automatic)
            .await
    }

    /// B3: only a real click may raise UAC.
    async fn start_for_user(&self) -> Result<()> {
        self.start_inner(crate::elevation_gate::ElevationTrigger::UserClick)
            .await
    }

    async fn stop(&self) -> Result<()> {
        // BUG 10 (Discord: ~380 ghost tray icons — a graceless `/F` kill
        // left the tray-icon shell notification undelivered, so Explorer
        // kept a stale icon around per kill). Graceful first: `/T` (whole
        // process tree — Chromium's renderer/GPU/utility children) WITHOUT
        // `/F`, giving the app a chance to run its own shutdown/tray-cleanup
        // path; wait up to 10s polling `is_running()`; escalate to `/T /F`
        // only if it is still alive after that window.
        #[cfg(target_os = "windows")]
        {
            let _ = crate::supervisor::platform::command("taskkill")
                .args(["/IM", "OlostepBrowser.exe", "/T"])
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT);

            let mut still_running = self.is_running();
            for _ in 0..10 {
                if !still_running {
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                still_running = self.is_running();
            }

            if !still_running {
                info!("Stopped OlostepBrowser (graceful)");
                return Ok(());
            }

            // Escalate: OlostepBrowser is Chromium-based and spawns a tree
            // of renderer/GPU/utility children. Killing only the parent
            // (no /T) left those alive, holding the profile lock and the
            // browser's own ports — field reports of "Olostep refuses to
            // shut down, requires a full reboot". /T kills the whole tree.
            warn!("OlostepBrowser did not exit gracefully within 10s — force-killing");
            let killed = crate::supervisor::platform::command("taskkill")
                .args(["/IM", "OlostepBrowser.exe", "/T", "/F"])
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT);
            match killed {
                // taskkill exits non-zero with "process not found" when nothing
                // was running, which is a successful stop, not a failure.
                Ok(o) if o.status.success() => info!("Stopped OlostepBrowser (forced)"),
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr);
                    if self.is_running() {
                        anyhow::bail!(
                            "OlostepBrowser is still running after taskkill: {}",
                            err.trim()
                        );
                    }
                    info!("OlostepBrowser was not running");
                }
                Err(e) => {
                    if self.is_running() {
                        anyhow::bail!("Failed to stop OlostepBrowser: {e}");
                    }
                    warn!(error = %e, "taskkill failed but OlostepBrowser is not running");
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            info!("Stopped OlostepBrowser");
        }
        // Whatever path stopped it, the tracked child (if any) is gone now.
        if let Ok(mut guard) = self.child.lock() {
            *guard = None;
        }
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        if self.is_running() {
            // BUG 10: resource caps (v0.4.8) used to report Unhealthy on a
            // breach, which the supervisor's restart path (stop → taskkill
            // → start) turned into a bounce — combined with the untracked
            // spawn/graceless-kill defects above, THIS was the mechanism
            // that produced ~380 ghost tray icons: a legitimate resource
            // spike triggered a kill the app didn't cleanly absorb, then a
            // respawn, repeatedly, at 30s health-tick cadence. Log only now
            // — never restart on a resource breach alone. The tracked child
            // + graceful stop above make an ACTUAL restart (e.g. a real
            // crash) safe when one is genuinely needed elsewhere; this path
            // specifically must not fire one.
            // block_in_place: the probe shells out to PowerShell (~<1s, but
            // it must not pin a tokio worker if PowerShell ever hangs).
            let breach = tokio::task::block_in_place(Self::resource_breach);
            if let Some(reason) = breach {
                warn!(reason = %reason, "OlostepBrowser resource cap breached — logging only, not restarting");
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
        let running = self.is_running();
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

/// BUG 10 (Discord: ~380 ghost tray icons) — tracked-child start guard,
/// fail-closed probe, graceful-then-forced stop.
#[cfg(test)]
mod bug10_tracked_child_tests {
    use super::*;

    fn spawn_long_lived() -> std::process::Child {
        #[cfg(target_os = "windows")]
        {
            crate::supervisor::platform::command("cmd")
                .args(["/C", "timeout /T 30 /NOBREAK >NUL"])
                .spawn()
                .expect("spawn cmd")
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::process::Command::new("sleep")
                .arg("30")
                .spawn()
                .expect("spawn sleep")
        }
    }

    fn spawn_short_lived() -> std::process::Child {
        #[cfg(target_os = "windows")]
        {
            crate::supervisor::platform::command("cmd")
                .args(["/C", "exit 0"])
                .spawn()
                .expect("spawn cmd")
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::process::Command::new("true")
                .spawn()
                .expect("spawn true")
        }
    }

    /// This is the guard `start()` relies on to never double-spawn: if the
    /// tracked child is still alive, `is_running()` must report `true`
    /// through the INSTANCE method (not just the shared free function) —
    /// proves AemIntegration's own wiring, not just `tracked_child_probe`
    /// in isolation.
    #[test]
    fn a_tracked_running_child_makes_is_running_true_so_start_would_not_double_spawn() {
        let integration = AemIntegration::default();
        let child = spawn_long_lived();
        *integration.child.lock().unwrap() = Some(child);

        assert!(
            integration.is_running(),
            "a live tracked child must report running"
        );

        // Clean up so the test doesn't leak a 30s sleep process.
        let taken = integration.child.lock().unwrap().take();
        if let Some(mut c) = taken {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    #[test]
    fn an_exited_tracked_child_clears_and_falls_back_to_the_image_name_probe() {
        let integration = AemIntegration::default();
        let mut child = spawn_short_lived();
        let _ = child.wait();
        *integration.child.lock().unwrap() = Some(child);

        // No real OlostepBrowser instance exists in this test environment,
        // so the image-name fallback should report not-running here —
        // proves the exited child does not get stuck reporting "running"
        // forever.
        let running = integration.is_running();
        assert!(
            !running,
            "an exited tracked child with no real instance running must report false"
        );
        assert!(
            integration.child.lock().unwrap().is_none(),
            "the exited child must be cleared from the tracked slot"
        );
    }

    /// Regression tripwire (same source-scan technique as
    /// commands/updates.rs::manual_install_tests): a probe timeout or error
    /// must read as "assume running", never "assume not running" — the
    /// exact combination that let a slow/failed tasklist probe spawn a
    /// duplicate OlostepBrowser on top of one that was actually alive.
    #[test]
    fn the_image_name_probe_fails_closed_not_open() {
        let src = include_str!("aem.rs");
        let after_probe_fn = src
            .split("fn image_name_probe()")
            .nth(1)
            .expect("image_name_probe must exist");
        let probe_body = after_probe_fn
            .split("fn is_running")
            .next()
            .unwrap_or(after_probe_fn);
        assert!(
            probe_body.contains("unwrap_or(true)"),
            "image_name_probe must fail CLOSED (unwrap_or(true)) on a probe error/timeout"
        );
        assert!(
            !probe_body.contains("unwrap_or(false)"),
            "image_name_probe must not fail OPEN (unwrap_or(false)) — the exact defect BUG 10 fixed"
        );
    }

    /// Regression tripwire: a resource breach must never return `Unhealthy`
    /// (which the supervisor's health loop turns into a restart) — that
    /// restart-on-breach cycle, combined with the untracked spawn/graceless
    /// kill this same bug fixed, is what produced ~380 ghost tray icons.
    #[test]
    fn health_check_never_returns_unhealthy_for_a_resource_breach() {
        let src = include_str!("aem.rs");
        let health_check_fn = src
            .split("async fn health_check(&self)")
            .nth(1)
            .expect("health_check must exist");
        let fn_body = health_check_fn
            .split("async fn check_update")
            .next()
            .unwrap_or(health_check_fn);
        assert!(
            !fn_body.contains("return HealthStatus::Unhealthy(reason)"),
            "a resource breach must be logged only, never trigger a restart via Unhealthy"
        );
    }
}
