use super::download::{download_file_with_options, partners_base_dir};
use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{info, warn};

const DOCKER_DOWNLOAD_URL: &str = "https://desktop.docker.com/win/main/amd64/Docker%20Desktop%20Installer.exe";
const DOCKER_PATHS: &[&str] = &[
    "C:\\Program Files\\Docker\\Docker\\Docker Desktop.exe",
    "C:\\Program Files (x86)\\Docker\\Docker\\Docker Desktop.exe",
    "C:\\Program Files\\Docker\\Docker\\Docker.exe",
    "C:\\Program Files (x86)\\Docker\\Docker\\Docker.exe",
];

/// Docker Desktop virtualization troubleshooting guide shown to users when
/// VT-x/AMD-V is disabled in firmware.
pub const VIRTUALIZATION_HELP_URL: &str =
    "https://docs.docker.com/desktop/troubleshoot-and-support/troubleshoot/topics/";

/// Distinct Docker runtime states. `docker info` alone cannot distinguish
/// "not installed" from "daemon stopped" — callers need the difference to
/// show accurate guidance instead of a generic timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DockerStatus {
    Ready,
    DaemonStopped,
    NotInstalled,
    VirtualizationDisabled,
}

/// Probe the docker CLI: Some(true) = daemon ready, Some(false) = CLI present
/// but daemon unreachable, None = CLI missing entirely (spawn failed).
fn docker_cli_probe() -> Option<bool> {
    match crate::supervisor::platform::command("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(s) if s.success() => Some(true),
        Ok(_) => Some(false),
        Err(_) => None,
    }
}

/// Check if Docker daemon is running by attempting `docker info`.
fn docker_running() -> bool {
    docker_cli_probe() == Some(true)
}

/// Whether the CPU/firmware can run Docker Desktop: true when a hypervisor is
/// already active (Hyper-V/WSL2) OR VT-x/AMD-V is enabled in firmware.
/// Fail-open on probe errors — an unreadable probe must not block installs.
/// Cached: the CIM query costs ~1-2s and firmware state can't change while
/// the app is running.
pub fn virtualization_supported() -> bool {
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| {
        #[cfg(target_os = "windows")]
        {
            let out = crate::supervisor::platform::command("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    "$h=(Get-CimInstance Win32_ComputerSystem).HypervisorPresent; \
                     $f=(Get-CimInstance Win32_Processor | Select-Object -First 1).VirtualizationFirmwareEnabled; \
                     Write-Output \"$h|$f\"",
                ])
                .output();
            match out {
                Ok(o) => {
                    let s = String::from_utf8_lossy(&o.stdout).trim().to_lowercase();
                    let mut parts = s.split('|');
                    let hypervisor = parts.next().unwrap_or("").contains("true");
                    let firmware = parts.next().unwrap_or("").contains("true");
                    if s.is_empty() || !s.contains('|') {
                        warn!(raw = %s, "Virtualization probe returned unparseable output — assuming supported");
                        true
                    } else {
                        let supported = hypervisor || firmware;
                        info!(hypervisor, firmware, supported, "Virtualization probe");
                        supported
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Virtualization probe failed — assuming supported");
                    true
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            true
        }
    })
}

/// Resolve the current Docker state, including whether virtualization makes
/// Docker viable at all on this machine.
pub fn docker_status() -> DockerStatus {
    match docker_cli_probe() {
        Some(true) => DockerStatus::Ready,
        Some(false) => {
            if virtualization_supported() {
                DockerStatus::DaemonStopped
            } else {
                DockerStatus::VirtualizationDisabled
            }
        }
        None => {
            // CLI missing — Docker Desktop may still be installed (PATH not
            // refreshed) or absent entirely.
            let installed = find_docker_desktop().is_some();
            if !virtualization_supported() {
                DockerStatus::VirtualizationDisabled
            } else if installed {
                DockerStatus::DaemonStopped
            } else {
                DockerStatus::NotInstalled
            }
        }
    }
}

/// User-facing guidance per Docker state. Non-Docker integrations
/// (MystNodes, SpaceAcres, Olostep) are unaffected by any of these states.
pub fn status_user_message(status: DockerStatus) -> String {
    match status {
        DockerStatus::Ready => "Docker is running.".to_string(),
        DockerStatus::DaemonStopped => {
            "Docker Desktop is installed but not running. Enabling a Docker-based integration will start it automatically, or open Docker Desktop from the Start menu and wait for the engine to report Running.".to_string()
        }
        DockerStatus::NotInstalled => {
            "Docker Desktop is not installed. Enabling a Docker-based integration (Presearch, Diiisco) will download and install it automatically (administrator approval required), or install it manually from https://www.docker.com/products/docker-desktop/.".to_string()
        }
        DockerStatus::VirtualizationDisabled => format!(
            "Hardware virtualization is disabled on this PC, so Docker (required by Presearch and Diiisco) cannot run. Enable virtualization (Intel VT-x / AMD-V / SVM) in your BIOS/UEFI settings, then try again. Guide: {} — Other integrations (MystNodes, SpaceAcres, Olostep) do not need Docker and keep working.",
            VIRTUALIZATION_HELP_URL
        ),
    }
}

/// Emit a docker-progress event to the frontend (no-op before setup).
fn emit_progress(stage: &str, detail: String, attempt: u32, total: u32) {
    crate::events::emit(
        "docker-progress",
        serde_json::json!({
            "stage": stage,
            "detail": detail,
            "attempt": attempt,
            "total": total,
        }),
    );
}

/// Scan Docker Desktop logs for specific known startup errors and return
/// a targeted user-facing message if found. Returns None for generic failures.
fn detect_docker_startup_error() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        let log_dir = dirs::data_local_dir()?.join("Docker").join("log");
        for entry in std::fs::read_dir(&log_dir).ok()? {
            let path = entry.ok()?.path();
            if path.extension().map_or(true, |e| e != "log" && e != "txt") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                // Check last 8KB only (recent entries)
                let tail = if content.len() > 8192 { &content[content.len() - 8192..] } else { &content };
                if tail.contains("hosts' is denied") || tail.contains("Access to the path") && tail.contains("drivers\\etc\\hosts") {
                    return Some(
                        "Docker Desktop cannot start because it cannot access your system hosts file. \
                         Fix: right-click Docker Desktop → 'Run as administrator', or fix the file permissions \
                         by running this command in an Administrator PowerShell:\n\
                         icacls C:\\WINDOWS\\System32\\drivers\\etc\\hosts /grant Users:RW\n\
                         Then re-enable this integration.".to_string()
                    );
                }
            }
        }
    }
    None
}

/// Find Docker Desktop executable in standard Windows paths.
fn find_docker_desktop() -> Option<PathBuf> {
    for path in DOCKER_PATHS {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Try to start Docker Desktop on Windows.
fn try_start_docker_desktop() -> Result<()> {
    let docker_exe = find_docker_desktop()
        .ok_or_else(|| anyhow::anyhow!("Docker Desktop not found in standard paths"))?;

    info!(path = ?docker_exe, "Starting Docker Desktop");
    crate::supervisor::platform::command(&docker_exe).spawn()?;
    Ok(())
}

/// Poll for Docker daemon availability, reporting progress to the UI.
async fn wait_for_docker(attempts: u32, delay_secs: u64) -> Result<()> {
    for attempt in 1..=attempts {
        if docker_running() {
            info!("Docker daemon is ready");
            emit_progress("ready", "Docker engine is ready".to_string(), attempt, attempts);
            return Ok(());
        }
        if attempt < attempts {
            info!(attempt = attempt, remaining = attempts - attempt, "Waiting for Docker daemon...");
            emit_progress(
                "waiting",
                format!("Waiting for the Docker engine to start ({}/{})", attempt, attempts),
                attempt,
                attempts,
            );
            tokio::time::sleep(Duration::from_secs(delay_secs)).await;
        }
    }
    anyhow::bail!(
        "Docker engine did not become ready within {} seconds",
        (attempts as u64) * delay_secs
    )
}

/// Download Docker Desktop installer to the partners directory.
async fn download_docker_installer() -> Result<PathBuf> {
    let install_dir = partners_base_dir().join("docker");
    std::fs::create_dir_all(&install_dir)?;
    let installer_path = install_dir.join("Docker-Desktop-Installer.exe");

    info!("Downloading Docker Desktop installer");
    emit_progress(
        "downloading",
        "Downloading Docker Desktop installer (~500 MB)".to_string(),
        0,
        0,
    );
    download_file_with_options(DOCKER_DOWNLOAD_URL, &installer_path, "", None)
        .await
        .map_err(|e| anyhow::anyhow!("Docker Desktop download failed: {}", e))?;
    info!(path = ?installer_path, "Docker Desktop installer downloaded");
    Ok(installer_path)
}

/// Run Docker Desktop installer with elevation and silent flags.
async fn run_docker_installer(installer_path: &std::path::Path) -> Result<()> {
    info!(path = ?installer_path, "Running Docker Desktop installer with elevation");
    emit_progress(
        "installing",
        "Installing Docker Desktop — approve the administrator prompt if shown".to_string(),
        0,
        0,
    );

    // On Windows, use ShellExecute via std::process with 'runas' verb via windows crate.
    // For simplicity and broad compatibility, we use a PowerShell wrapper that handles elevation.
    let ps_script = format!(
        r#"
$installer = '{}'
$args = @('install', '--quiet', '--accept-license')
Start-Process -FilePath $installer -ArgumentList $args -Verb RunAs -Wait
Exit $LASTEXITCODE
"#,
        installer_path.display()
    );

    let output = crate::supervisor::platform::command("powershell")
        .arg("-NoProfile")
        .arg("-Command")
        .arg(&ps_script)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Check for UAC denial (common error code 1223)
        if output.status.code() == Some(1223) {
            anyhow::bail!("Docker Desktop installation requires administrator permission. Please install Docker Desktop manually from docker.com or re-run FEM as administrator.");
        }

        warn!(code = output.status.code(), stderr = %stderr, "Docker Desktop installer failed");
        anyhow::bail!(
            "Docker Desktop installation failed (exit code {}): {}",
            output.status.code().unwrap_or(-1),
            stderr
        );
    }

    info!("Docker Desktop installer completed");
    Ok(())
}

/// Ensure Docker is available and running.
///
/// State-aware flow:
/// - Ready → Ok immediately.
/// - VirtualizationDisabled → fail fast with BIOS guidance (installing or
///   starting Docker would be pointless).
/// - DaemonStopped → start Docker Desktop, wait with UI progress.
/// - NotInstalled → download installer, elevate, install, wait (first engine
///   start after install is slow — extended timeout).
///
/// Only call on an explicit user action (toggle/install) — never at app boot.
pub async fn ensure_docker() -> Result<()> {
    match docker_status() {
        DockerStatus::Ready => {
            info!("Docker is already available");
            Ok(())
        }
        DockerStatus::VirtualizationDisabled => {
            warn!("Virtualization disabled — cannot provide Docker");
            anyhow::bail!(status_user_message(DockerStatus::VirtualizationDisabled))
        }
        DockerStatus::DaemonStopped => {
            info!("Docker Desktop installed but engine not running — starting it");
            emit_progress(
                "starting",
                "Starting Docker Desktop…".to_string(),
                0,
                0,
            );
            if let Err(e) = try_start_docker_desktop() {
                warn!(error = %e, "Failed to launch Docker Desktop");
            }
            wait_for_docker(30, 5).await.map_err(|e| {
                // Check Docker Desktop logs for specific known errors
                if let Some(specific) = detect_docker_startup_error() {
                    return anyhow::anyhow!("{}", specific);
                }
                anyhow::anyhow!(
                    "{}. Docker Desktop is installed but its engine did not start. Open Docker Desktop from the Start menu and wait for the engine to report Running, then re-enable this integration. If Docker Desktop shows a virtualization error, enable VT-x/AMD-V in your BIOS: {}",
                    e,
                    VIRTUALIZATION_HELP_URL
                )
            })
        }
        DockerStatus::NotInstalled => {
            info!("Docker Desktop not installed — downloading installer");
            let installer_path = download_docker_installer().await?;
            run_docker_installer(&installer_path).await?;
            std::fs::remove_file(&installer_path).ok();

            // Fresh installs may need a first-run engine bootstrap; some
            // machines require the Desktop app to be launched explicitly.
            if !docker_running() {
                let _ = try_start_docker_desktop();
            }
            wait_for_docker(60, 5).await.map_err(|e| {
                anyhow::anyhow!(
                    "Docker Desktop was installed, but {}. A restart of Windows may be required to finish setup (WSL2/Hyper-V components). Restart, open Docker Desktop once, then re-enable this integration.",
                    e
                )
            })?;
            info!("Docker Desktop installation and startup complete");
            Ok(())
        }
    }
}
