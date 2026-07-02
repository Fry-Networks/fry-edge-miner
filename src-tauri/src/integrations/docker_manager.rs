use super::download::{download_file_with_options, partners_base_dir};
use anyhow::Result;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{info, warn};

const DOCKER_DOWNLOAD_URL: &str = "https://desktop.docker.com/win/main/amd64/Docker%20Desktop%20Installer.exe";
const DOCKER_PATHS: &[&str] = &[
    "C:\\Program Files\\Docker\\Docker\\Docker.exe",
    "C:\\Program Files (x86)\\Docker\\Docker\\Docker.exe",
];

/// Check if Docker daemon is running by attempting `docker info`.
fn docker_running() -> bool {
    crate::supervisor::platform::command("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
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

/// Poll for Docker daemon availability.
async fn wait_for_docker(attempts: u32, delay_secs: u64) -> Result<()> {
    for attempt in 1..=attempts {
        if docker_running() {
            info!("Docker daemon is ready");
            return Ok(());
        }
        if attempt < attempts {
            info!(attempt = attempt, remaining = attempts - attempt, "Waiting for Docker daemon...");
            tokio::time::sleep(Duration::from_secs(delay_secs)).await;
        }
    }
    anyhow::bail!(
        "Docker daemon did not become available after {} attempts ({} seconds total)",
        attempts,
        (attempts as u64) * delay_secs
    )
}

/// Download Docker Desktop installer to the partners directory.
async fn download_docker_installer() -> Result<PathBuf> {
    let install_dir = partners_base_dir().join("docker");
    std::fs::create_dir_all(&install_dir)?;
    let installer_path = install_dir.join("Docker-Desktop-Installer.exe");

    info!("Downloading Docker Desktop installer");
    download_file_with_options(DOCKER_DOWNLOAD_URL, &installer_path, "", None).await?;
    info!(path = ?installer_path, "Docker Desktop installer downloaded");
    Ok(installer_path)
}

/// Run Docker Desktop installer with elevation and silent flags.
async fn run_docker_installer(installer_path: &std::path::Path) -> Result<()> {
    info!(path = ?installer_path, "Running Docker Desktop installer with elevation");

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
/// Checks if `docker info` works. If not:
/// 1. If Docker Desktop is installed, try to start it and wait for daemon.
/// 2. If not installed, download the installer, request elevation, run it, and wait.
/// 3. If elevation is denied (UAC), return a user-friendly error.
pub async fn ensure_docker() -> Result<()> {
    // Quick check: is Docker already running?
    if docker_running() {
        info!("Docker is already available");
        return Ok(());
    }

    info!("Docker not available, attempting to ensure it...");

    // Check if Docker Desktop binary exists (may be installed but daemon not running)
    if let Some(docker_exe) = find_docker_desktop() {
        info!(path = ?docker_exe, "Docker Desktop is installed, attempting to start daemon");
        if let Err(e) = try_start_docker_desktop() {
            warn!(error = %e, "Failed to start Docker Desktop");
        }

        // Wait for daemon to be ready
        match wait_for_docker(30, 5).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                warn!(error = %e, "Docker daemon did not become available after starting Docker Desktop");
                // Fall through to installer flow
            }
        }
    }

    // Docker is not installed or failed to start — download and install
    info!("Docker Desktop not installed or failed to start, downloading installer");
    let installer_path = download_docker_installer().await?;

    // Run installer with elevation
    run_docker_installer(&installer_path).await?;

    // Clean up installer
    std::fs::remove_file(&installer_path).ok();

    // Wait for Docker daemon to be ready after installation
    wait_for_docker(30, 5).await?;

    info!("Docker Desktop installation and startup complete");
    Ok(())
}
