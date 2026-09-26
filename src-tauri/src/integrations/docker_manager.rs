use super::download::{download_file_with_options, partners_base_dir};
use crate::supervisor::platform::BoundedOutput;
use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tracing::{info, warn};

const DOCKER_DOWNLOAD_URL: &str =
    "https://desktop.docker.com/win/main/amd64/Docker%20Desktop%20Installer.exe";
const DOCKER_PATHS: &[&str] = &[
    "C:\\Program Files\\Docker\\Docker\\Docker Desktop.exe",
    "C:\\Program Files (x86)\\Docker\\Docker\\Docker Desktop.exe",
    "C:\\Program Files\\Docker\\Docker\\Docker.exe",
    "C:\\Program Files (x86)\\Docker\\Docker\\Docker.exe",
];

/// Docker Desktop 4.30+ can install per-user, outside Program Files. These are
/// relative to %LOCALAPPDATA% / %PROGRAMDATA% and are resolved at runtime.
const DOCKER_USER_SCOPE_PATHS: &[(&str, &str)] = &[
    (
        "LOCALAPPDATA",
        "Programs\\Docker\\Docker\\Docker Desktop.exe",
    ),
    ("LOCALAPPDATA", "Programs\\Docker\\Docker\\Docker.exe"),
    ("ProgramData", "DockerDesktop\\Docker Desktop.exe"),
];

/// Named pipe the Docker engine listens on. Its presence proves Docker is
/// installed even when the CLI is absent from this process's PATH.
#[cfg(target_os = "windows")]
const DOCKER_ENGINE_PIPE: &str = "\\\\.\\pipe\\docker_engine";

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

/// Outcome of the bounded `docker info` probe. A TIMEOUT and a MISSING CLI are
/// different facts: a busy daemon (many running containers) routinely takes
/// longer than the probe window, and collapsing that into "CLI missing" is what
/// made FEM report "Docker is not installed" on machines where Docker Desktop
/// was visibly running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockerProbe {
    /// `docker info` exited 0 — daemon reachable.
    Ready,
    /// CLI ran but exited non-zero — installed, daemon not serving.
    DaemonUnreachable,
    /// CLI ran but did not finish inside the probe window. Says nothing about
    /// whether Docker is installed.
    TimedOut,
    /// The `docker` binary could not be spawned at all.
    CliMissing,
}

/// PURE: which `docker` executable to spawn.
///
/// `None` means "keep using the bare name". Every one of the 25 docker spawn
/// sites resolved `docker` by bare name against the PATH this PROCESS was
/// launched with, and Windows does not refresh a running process's
/// environment. Installing Docker — or fixing its PATH entry — while FEM was
/// already running therefore left it permanently CliMissing, and the only
/// cure was restarting the app, which is exactly what users found.
pub(crate) fn pick_docker_cli(on_path: bool, candidates: &[PathBuf]) -> Option<PathBuf> {
    if on_path {
        return None;
    }
    candidates.iter().find(|p| p.exists()).cloned()
}

/// `...\Docker\Docker\Docker Desktop.exe` -> `...\Docker\Docker\resources\bin\docker.exe`
pub(crate) fn cli_beside_desktop_exe(desktop: &std::path::Path) -> Option<PathBuf> {
    desktop
        .parent()
        .map(|dir| dir.join("resources").join("bin").join("docker.exe"))
}

/// Absolute `docker.exe` locations worth trying, derived from the SAME install
/// paths the installed-check already uses.
#[cfg(target_os = "windows")]
fn docker_cli_candidates() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for p in DOCKER_PATHS {
        if let Some(cli) = cli_beside_desktop_exe(std::path::Path::new(p)) {
            out.push(cli);
        }
    }
    for (var, rel) in DOCKER_USER_SCOPE_PATHS {
        if let Ok(base) = std::env::var(var) {
            if let Some(cli) = cli_beside_desktop_exe(&PathBuf::from(base).join(rel)) {
                out.push(cli);
            }
        }
    }
    out.dedup();
    out
}

#[cfg(not(target_os = "windows"))]
fn docker_cli_candidates() -> Vec<PathBuf> {
    Vec::new()
}

static DOCKER_CLI_CACHE: std::sync::Mutex<Option<(Option<PathBuf>, std::time::Instant)>> =
    std::sync::Mutex::new(None);

/// Forget the resolved CLI so the next spawn re-resolves it. Called whenever a
/// spawn fails, so a path that stops working is not cached until the TTL.
fn invalidate_docker_cli() {
    if let Ok(mut guard) = DOCKER_CLI_CACHE.lock() {
        *guard = None;
    }
}

/// Whether the PREVIOUS `docker_bounded_probe` CLI-spawn attempt also failed.
/// Read-and-reset around each attempt so a run of identical failures logs the
/// fact once, not once per health tick (FAIL-14).
static DOCKER_SPAWN_FAILED: AtomicBool = AtomicBool::new(false);

/// PURE: should THIS spawn failure be logged at WARN, given whether the
/// previous probe's spawn also failed?
///
/// Only the transition INTO failure earns a WARN — a run of N consecutive
/// failures logs exactly one. A success in between means the NEXT failure is
/// a new fact, not a repeat, and must warn again.
pub(crate) fn should_warn_on_spawn_failure(previous_spawn_failed: bool) -> bool {
    !previous_spawn_failed
}

/// A `Command` for the docker CLI, resolved to an absolute path when the bare
/// name is not spawnable. Drop-in for `platform::command("docker")`.
pub fn docker_command() -> std::process::Command {
    if let Ok(guard) = DOCKER_CLI_CACHE.lock() {
        if let Some((resolved, at)) = guard.as_ref() {
            if cache_is_fresh(Some(*at), std::time::Instant::now(), VIRT_CACHE_TTL) {
                return match resolved {
                    Some(p) => crate::supervisor::platform::command(p),
                    None => crate::supervisor::platform::command("docker"),
                };
            }
        }
    }

    let on_path = crate::supervisor::platform::command("docker")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|mut c| {
            let _ = c.wait();
            true
        })
        .unwrap_or(false);

    let resolved = pick_docker_cli(on_path, &docker_cli_candidates());
    if let Some(path) = resolved.as_ref() {
        warn!(path = ?path, "Docker CLI is not on this process's PATH — using the installed copy");
    }
    if let Ok(mut guard) = DOCKER_CLI_CACHE.lock() {
        *guard = Some((resolved.clone(), std::time::Instant::now()));
    }
    match resolved {
        Some(p) => crate::supervisor::platform::command(p),
        None => crate::supervisor::platform::command("docker"),
    }
}

/// Spawn a bounded `docker <args...>` probe, polling every 100ms up to
/// `timeout_secs`, killing and reaping the child on timeout.
fn docker_bounded_probe(args: &[&str], timeout_secs: u64) -> DockerProbe {
    const POLL_INTERVAL_MS: u64 = 100;
    let max_polls = (timeout_secs * 1000) / POLL_INTERVAL_MS;

    let mut cmd = docker_command();
    for a in args {
        cmd.arg(a);
    }
    let mut child = match cmd
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => {
            // A spawn that works is a state change from a prior failure —
            // the NEXT failure (if any) is a new fact again, so re-arm the
            // warning.
            DOCKER_SPAWN_FAILED.store(false, Ordering::Relaxed);
            c
        }
        Err(e) => {
            // FAIL-14: this used to warn on EVERY failed probe — 175
            // identical "Docker CLI could not be spawned" lines in a single
            // 76-minute soak, one per health tick. Only the transition INTO
            // failure is worth a WARN; a run of repeats is downgraded to
            // debug so the fact isn't lost, just not repeated.
            let previously_failed = DOCKER_SPAWN_FAILED.swap(true, Ordering::Relaxed);
            if should_warn_on_spawn_failure(previously_failed) {
                warn!(error = %e, "Docker CLI could not be spawned");
            } else {
                tracing::debug!(error = %e, "Docker CLI could not be spawned (repeat)");
            }
            invalidate_docker_cli();
            return DockerProbe::CliMissing;
        }
    };

    for _poll in 0..max_polls {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    DockerProbe::Ready
                } else {
                    DockerProbe::DaemonUnreachable
                }
            }
            Ok(None) => {
                std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
            }
            Err(_) => return DockerProbe::CliMissing,
        }
    }

    // Timeout reached — kill and reap the child so it can't linger as a zombie.
    let _ = child.kill();
    let _ = child.wait();
    DockerProbe::TimedOut
}

/// Bounded docker probe. `docker info` is authoritative but heavy: on a busy
/// Docker Desktop (WSL2) daemon it regularly took longer than the old 3s window
/// and was reported as Stopped even though `docker run hello-world` worked (F11).
/// Give `docker info` a longer window and, on timeout, fall back to the far
/// lighter `docker version` server ping before concluding the daemon is down.
pub fn docker_cli_probe_detailed() -> DockerProbe {
    match docker_bounded_probe(&["info"], 10) {
        DockerProbe::TimedOut => {
            match docker_bounded_probe(&["version", "--format", "{{.Server.Version}}"], 5) {
                // A successful server-version ping means the daemon is reachable.
                DockerProbe::Ready => DockerProbe::Ready,
                _ => {
                    tracing::warn!(
                        "Docker probe timed out and the lighter version ping did not confirm the daemon"
                    );
                    DockerProbe::TimedOut
                }
            }
        }
        other => other,
    }
}

/// Probe the docker CLI: Some(true) = daemon ready, Some(false) = CLI present
/// but daemon unreachable, None = CLI missing or unresponsive.
/// Uses bounded probe with 3-second timeout to prevent hangs.
pub fn docker_cli_probe_bounded() -> Option<bool> {
    match docker_cli_probe_detailed() {
        DockerProbe::Ready => Some(true),
        DockerProbe::DaemonUnreachable => Some(false),
        DockerProbe::TimedOut | DockerProbe::CliMissing => None,
    }
}

/// Check if Docker daemon is running by attempting `docker info`.
fn docker_running() -> bool {
    docker_cli_probe_detailed() == DockerProbe::Ready
}

/// Whether the CPU/firmware can run Docker Desktop: true when a hypervisor is
/// already active (Hyper-V/WSL2) OR VT-x/AMD-V is enabled in firmware.
/// Fail-open on probe errors — an unreadable probe must not block installs.
/// Cached: the CIM query costs ~1-2s and firmware state can't change while
/// the app is running.
pub fn virtualization_supported() -> bool {
    if let Ok(guard) = VIRT_CACHE.lock() {
        if let Some((value, at)) = *guard {
            if cache_is_fresh(Some(at), std::time::Instant::now(), VIRT_CACHE_TTL) {
                return value;
            }
        }
    }
    let value = probe_virtualization_supported();
    if let Ok(mut guard) = VIRT_CACHE.lock() {
        *guard = Some((value, std::time::Instant::now()));
    }
    value
}

/// How long a virtualization reading is trusted. Same shape and duration as
/// the SSD probe cache in `space_acres.rs`.
const VIRT_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(600);

static VIRT_CACHE: std::sync::Mutex<Option<(bool, std::time::Instant)>> =
    std::sync::Mutex::new(None);

/// PURE: is a cached reading still good?
///
/// The old cache was a process-lifetime `OnceLock`, so a single sample taken
/// before WSL2/Hyper-V had finished coming up was never re-probed for the life
/// of the app — and that reading is serialised straight to the UI. A TTL can
/// only turn a stale `false` into a fresh `true`, so nothing tightens.
pub(crate) fn cache_is_fresh(
    recorded: Option<std::time::Instant>,
    now: std::time::Instant,
    ttl: std::time::Duration,
) -> bool {
    match recorded {
        Some(at) => now.duration_since(at) < ttl,
        None => false,
    }
}

fn probe_virtualization_supported() -> bool {
    {
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
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT);
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
    }
}

/// Whether this Windows install is itself a VM guest (Proxmox/KVM, VMware,
/// VirtualBox, Hyper-V, Xen). Matters for the VirtualizationDisabled
/// guidance: inside a guest the fix is nested virtualization on the VM HOST,
/// not a BIOS setting — telling a VM user to "enable VT-x in your BIOS" sends
/// them somewhere the switch does not exist (v0.4.7 Proxmox field reports).
/// Cached; fail-closed to false (bare-metal guidance) on probe errors.
pub fn is_virtual_machine() -> bool {
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| {
        #[cfg(target_os = "windows")]
        {
            let out = crate::supervisor::platform::command("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    "$c=Get-CimInstance Win32_ComputerSystem; Write-Output \"$($c.Manufacturer)|$($c.Model)\"",
                ])
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT);
            match out {
                Ok(o) => {
                    let s = String::from_utf8_lossy(&o.stdout).trim().to_lowercase();
                    let vm = ["qemu", "kvm", "proxmox", "vmware", "virtualbox", "vbox", "xen", "virtual machine", "hyper-v"]
                        .iter()
                        .any(|m| s.contains(m));
                    info!(raw = %s, vm, "VM guest probe");
                    vm
                }
                Err(e) => {
                    warn!(error = %e, "VM guest probe failed — assuming bare metal");
                    false
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    })
}

/// Resolve the current Docker state, including whether virtualization makes
/// Docker viable at all on this machine.
pub fn docker_status() -> DockerStatus {
    resolve_docker_status(
        docker_cli_probe_detailed(),
        docker_desktop_installed(),
        virtualization_supported(),
    )
}

/// Testable core of `docker_status`. Kept pure so the state machine can be
/// exercised without a real Docker install.
///
/// The load-bearing rule: only `CliMissing` + no install evidence may ever
/// produce `NotInstalled`. A probe TIMEOUT means "we don't know", and claiming
/// "Docker is not installed" on a machine that is running containers is worse
/// than saying the daemon isn't answering.
pub fn resolve_docker_status(
    probe: DockerProbe,
    installed: bool,
    virtualization: bool,
) -> DockerStatus {
    match probe {
        DockerProbe::Ready => DockerStatus::Ready,
        DockerProbe::DaemonUnreachable | DockerProbe::TimedOut => {
            // The CLI exists (it ran), so Docker is installed by definition.
            if virtualization {
                DockerStatus::DaemonStopped
            } else {
                DockerStatus::VirtualizationDisabled
            }
        }
        DockerProbe::CliMissing => {
            if installed {
                // Installed but not on this process's PATH (common right after
                // an install, before the environment is refreshed).
                //
                // Checked BEFORE the firmware probe on purpose: `installed` is
                // hard evidence — the engine's named pipe, or Docker Desktop's
                // uninstall key — while the probe is a CIM query that fails
                // open and can be stale. Testing the probe first let a machine
                // with Docker demonstrably present be reported as
                // "Docker unavailable", which also made ensure_docker bail
                // with BIOS guidance and left the watcher's auto-start
                // unarmed.
                DockerStatus::DaemonStopped
            } else if !virtualization {
                DockerStatus::VirtualizationDisabled
            } else {
                DockerStatus::NotInstalled
            }
        }
    }
}

/// User-facing guidance per Docker state. Non-Docker integrations
/// (MystNodes, SpaceAcres, Olostep) are unaffected by any of these states.
pub fn status_user_message(status: DockerStatus) -> String {
    status_user_message_for(status, is_virtual_machine())
}

/// Testable core of status_user_message — `vm_guest` selects the right
/// VirtualizationDisabled guidance (VM host setting vs BIOS setting).
pub fn status_user_message_for(status: DockerStatus, vm_guest: bool) -> String {
    match status {
        DockerStatus::Ready => "Docker is running.".to_string(),
        DockerStatus::DaemonStopped => {
            "Docker Desktop is installed but not running. Enabling a Docker-based integration will start it automatically, or open Docker Desktop from the Start menu and wait for the engine to report Running.".to_string()
        }
        DockerStatus::NotInstalled => {
            "Docker Desktop is not installed. Enabling a Docker-based integration (Diiisco) will download and install it automatically (administrator approval required), or install it manually from https://www.docker.com/products/docker-desktop/.".to_string()
        }
        DockerStatus::VirtualizationDisabled if vm_guest => format!(
            "This Windows install is a virtual machine and nested virtualization is not exposed to it, so Docker cannot run here. Enable nested virtualization on the VM host (Proxmox/KVM: CPU type 'host'; VMware: 'Virtualize Intel VT-x/EPT'; Hyper-V: Set-VMProcessor -ExposeVirtualizationExtensions $true), then restart this VM. Guide: {} — Other integrations (MystNodes, SpaceAcres, Olostep) do not need Docker and keep working.",
            VIRTUALIZATION_HELP_URL
        ),
        DockerStatus::VirtualizationDisabled => format!(
            "Hardware virtualization is disabled on this PC, so Docker (required by Diiisco) cannot run. Enable virtualization (Intel VT-x / AMD-V / SVM) in your BIOS/UEFI settings, then try again. Guide: {} — Other integrations (MystNodes, SpaceAcres, Olostep) do not need Docker and keep working.",
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
            if path.extension().is_none_or(|e| e != "log" && e != "txt") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                // Check last 8KB only (recent entries)
                let tail = if content.len() > 8192 {
                    &content[content.len() - 8192..]
                } else {
                    &content
                };
                if tail.contains("hosts' is denied")
                    || tail.contains("Access to the path") && tail.contains("drivers\\etc\\hosts")
                {
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
    // Docker Desktop 4.30+ supports per-user installs outside Program Files.
    for (var, rel) in DOCKER_USER_SCOPE_PATHS {
        if let Ok(base) = std::env::var(var) {
            let p = PathBuf::from(base).join(rel);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

/// Whether Docker Desktop is installed, by any evidence we can gather without
/// the CLI: a known executable path (machine- or user-scope), the engine's
/// named pipe, or the Windows uninstall registry entry.
///
/// Deliberately broader than `find_docker_desktop`, which must return a path it
/// can actually launch; here a pipe or registry hit is enough to prove presence.
pub fn docker_desktop_installed() -> bool {
    if find_docker_desktop().is_some() {
        return true;
    }
    #[cfg(target_os = "windows")]
    {
        // The engine pipe only exists when Docker is installed and running —
        // the exact case that was being misreported as "not installed".
        if std::fs::metadata(DOCKER_ENGINE_PIPE).is_ok() {
            return true;
        }
        let out = crate::supervisor::platform::command("reg")
            .args([
                "query",
                "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Docker Desktop",
                "/v",
                "DisplayName",
            ])
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT);
        if let Ok(o) = out {
            if o.status.success() {
                return true;
            }
        }
    }
    false
}

/// Try to start Docker Desktop on Windows.
/// User-level spawn, NO UAC prompt. Safe to call from auto-healing paths.
pub(crate) fn try_start_docker_desktop() -> Result<()> {
    let docker_exe = find_docker_desktop()
        .ok_or_else(|| anyhow::anyhow!("Docker Desktop not found in standard paths"))?;

    info!(path = ?docker_exe, "Starting Docker Desktop");
    // B15 (D-13): Docker Desktop is a full third-party desktop app that owns
    // its own UI and spawns an engine/WSL tree. FEM now sets a PROCESS error
    // mode that children inherit, which is right for FEM-managed partners but
    // must not silence Docker's own dialogs — CREATE_DEFAULT_ERROR_MODE opts
    // this one launch out. It is the ONLY exemption: OlostepBrowser and
    // space-acres are FEM-managed partners and SHOULD inherit, which is the
    // Done-when's "spawned partners never raise system modal dialogs".
    let mut cmd = crate::supervisor::platform::command(&docker_exe);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_DEFAULT_ERROR_MODE: u32 = 0x0400_0000;
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_DEFAULT_ERROR_MODE);
    }
    cmd.spawn()?;
    Ok(())
}

/// Poll for Docker daemon availability, reporting progress to the UI.
async fn wait_for_docker(attempts: u32, delay_secs: u64) -> Result<()> {
    for attempt in 1..=attempts {
        if docker_running() {
            info!("Docker daemon is ready");
            emit_progress(
                "ready",
                "Docker engine is ready".to_string(),
                attempt,
                attempts,
            );
            return Ok(());
        }
        if attempt < attempts {
            info!(
                attempt = attempt,
                remaining = attempts - attempt,
                "Waiting for Docker daemon..."
            );
            emit_progress(
                "waiting",
                format!(
                    "Waiting for the Docker engine to start ({}/{})",
                    attempt, attempts
                ),
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
async fn run_docker_installer(
    installer_path: &std::path::Path,
    trigger: crate::elevation_gate::ElevationTrigger,
) -> Result<()> {
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
Start-Process -FilePath $installer -ArgumentList $args -Verb RunAs -WindowStyle Hidden -Wait
Exit $LASTEXITCODE
"#,
        installer_path.display()
    );

    // B3 / G4: this raised `Start-Process -Verb RunAs` directly, exactly like
    // titan's redist installer did, and referenced the elevation gate nowhere —
    // while the gate's own module doc listed this function as one of the five
    // sites it covered. `ensure_docker()` reaches it whenever Docker Desktop is
    // absent, and both the boot recovery pass and the Docker watcher call into
    // install()/start(), so a UAC dialog could appear with no user gesture.
    let attempt_key = format!(
        "docker-desktop|{}",
        installer_path.to_string_lossy().to_lowercase()
    );
    let gated =
        crate::elevation_gate::run_elevated("docker-desktop", &attempt_key, trigger, move || {
            crate::supervisor::platform::command("powershell")
                .arg("-NoProfile")
                .arg("-Command")
                .arg(&ps_script)
                .output_bounded(crate::supervisor::platform::LONG_TIMEOUT)
                .map_err(anyhow::Error::new)
        });
    let output = match gated {
        Ok(out) => out,
        Err(skipped) => {
            warn!(reason = %skipped, "Docker Desktop install skipped by the elevation gate");
            anyhow::bail!("{skipped}")
        }
    };

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
    // Automatic by default: every existing caller is reachable from the boot
    // recovery pass, a health tick or the Docker watcher, none of which is a
    // user gesture. A user-initiated enable calls `ensure_docker_with` first.
    ensure_docker_with(crate::elevation_gate::ElevationTrigger::Automatic).await
}

/// `ensure_docker`, with the elevation authority of whatever asked for it.
///
/// Idempotent: once Docker Desktop is installed and its engine is running this
/// returns immediately, which is what lets a user-initiated install satisfy
/// Docker with `UserClick` authority and then run the ordinary install path
/// unchanged.
pub async fn ensure_docker_with(trigger: crate::elevation_gate::ElevationTrigger) -> Result<()> {
    ensure_docker_core(trigger, true).await
}

/// FAIL-13: like `ensure_docker_with`, but NEVER downloads or installs Docker
/// Desktop — for automatic paths (a supervisor restart, a health tick) that
/// must not put a gesture-less network fetch on the wire, let alone an
/// elevated install.
///
/// `ensure_docker_with`'s `NotInstalled` branch downloaded the installer
/// BEFORE the elevation gate was ever consulted — only the install/elevate
/// step was gated, the download was not. A Pawns supervisor restart
/// (`stop_for_restart` -> `start()` -> this) with Docker absent therefore
/// reached a real network fetch with nobody at the keyboard. `Ready` /
/// `VirtualizationDisabled` / `DaemonStopped` are unaffected — none of them
/// ever downloads anything (`DaemonStopped` only launches the
/// ALREADY-INSTALLED app). A real user gesture still goes through
/// `ensure_docker_with(UserClick)`, which can install (see
/// `PawnsIntegration::start_for_user`, mirroring `install_for_user`).
pub async fn ensure_docker_no_install() -> Result<()> {
    ensure_docker_core(crate::elevation_gate::ElevationTrigger::Automatic, false).await
}

/// FAIL-13 (c4 BUG LOOP 4): may this caller fetch the Docker Desktop installer?
///
/// Only a real user gesture may. An Automatic caller (the boot recovery pass,
/// a supervisor restart, a health tick) is refused by the elevation gate at the
/// install step anyway, so a download on its behalf is a gesture-less ~600 MB
/// fetch that can never be used — RC13 repeated it every few minutes.
fn may_fetch_docker_installer(trigger: crate::elevation_gate::ElevationTrigger) -> bool {
    trigger == crate::elevation_gate::ElevationTrigger::UserClick
}

async fn ensure_docker_core(
    trigger: crate::elevation_gate::ElevationTrigger,
    allow_install: bool,
) -> Result<()> {
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
            emit_progress("starting", "Starting Docker Desktop…".to_string(), 0, 0);
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
            if !allow_install {
                anyhow::bail!(
                    "Docker Desktop is not installed — enable this integration once (or use \
                     its Settings action) to install it"
                );
            }
            // FAIL-13 (c4 BUG LOOP 4): `allow_install` alone is not a gesture —
            // `ensure_docker()` passes it on every automatic path, and this
            // arm fetched the installer before the gate refused to run it.
            if !may_fetch_docker_installer(trigger) {
                anyhow::bail!(
                    "Docker Desktop is not installed — enable this integration once (or use \
                     its Settings action) to install it"
                );
            }
            info!("Docker Desktop not installed — downloading installer");
            let installer_path = download_docker_installer().await?;
            run_docker_installer(&installer_path, trigger).await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    // v0.4.8: VirtualizationDisabled guidance must point VM guests at the
    // hypervisor host, not a BIOS switch the guest does not have (Proxmox
    // field reports: users were told to enable VT-x in a VM's "BIOS").
    #[test]
    fn vm_guest_gets_nested_virtualization_guidance() {
        let msg = status_user_message_for(DockerStatus::VirtualizationDisabled, true);
        assert!(msg.contains("virtual machine"), "got: {msg}");
        assert!(msg.contains("nested virtualization"), "got: {msg}");
        assert!(msg.to_lowercase().contains("proxmox"), "got: {msg}");
        assert!(
            !msg.contains("BIOS/UEFI settings"),
            "VM guests must not get the BIOS path: {msg}"
        );
    }

    #[test]
    fn bare_metal_keeps_the_bios_guidance() {
        let msg = status_user_message_for(DockerStatus::VirtualizationDisabled, false);
        assert!(msg.contains("BIOS/UEFI"), "got: {msg}");
        assert!(!msg.contains("nested virtualization"), "got: {msg}");
    }

    // Field reports (georgeparis, Jesco39967): FEM said "Docker Desktop is not
    // installed" in screenshots showing Docker Desktop running with containers.
    // A busy daemon makes `docker info` exceed the 3s probe window; that timeout
    // used to be indistinguishable from "CLI missing", which then fell through
    // to a Program-Files-only path check and reported NotInstalled.
    #[test]
    fn probe_timeout_never_reports_docker_missing() {
        // Worst case for the old code: timed out AND no install path found.
        assert_eq!(
            resolve_docker_status(DockerProbe::TimedOut, false, true),
            DockerStatus::DaemonStopped,
            "a probe timeout must not be reported as NotInstalled"
        );
        assert_eq!(
            resolve_docker_status(DockerProbe::TimedOut, true, true),
            DockerStatus::DaemonStopped
        );
    }

    #[test]
    fn cli_missing_but_installed_reports_daemon_stopped() {
        // User-scope install (%LOCALAPPDATA%\Programs\Docker) with docker not on PATH.
        assert_eq!(
            resolve_docker_status(DockerProbe::CliMissing, true, true),
            DockerStatus::DaemonStopped
        );
    }

    #[test]
    fn only_a_missing_cli_with_no_install_evidence_is_not_installed() {
        assert_eq!(
            resolve_docker_status(DockerProbe::CliMissing, false, true),
            DockerStatus::NotInstalled
        );
    }

    #[test]
    fn ready_probe_is_ready_regardless_of_path_detection() {
        assert_eq!(
            resolve_docker_status(DockerProbe::Ready, false, true),
            DockerStatus::Ready
        );
    }

    #[test]
    fn virtualization_disabled_still_wins_where_it_applies() {
        assert_eq!(
            resolve_docker_status(DockerProbe::DaemonUnreachable, true, false),
            DockerStatus::VirtualizationDisabled
        );
        assert_eq!(
            resolve_docker_status(DockerProbe::CliMissing, false, false),
            DockerStatus::VirtualizationDisabled
        );
    }

    // The 4 integrations gate on `docker_cli_probe_bounded() == Some(true)`;
    // that contract must survive the probe refactor.
    #[test]
    fn bounded_probe_keeps_its_option_contract() {
        for (probe, expected) in [
            (DockerProbe::Ready, Some(true)),
            (DockerProbe::DaemonUnreachable, Some(false)),
            (DockerProbe::TimedOut, None),
            (DockerProbe::CliMissing, None),
        ] {
            let mapped = match probe {
                DockerProbe::Ready => Some(true),
                DockerProbe::DaemonUnreachable => Some(false),
                DockerProbe::TimedOut | DockerProbe::CliMissing => None,
            };
            assert_eq!(mapped, expected, "probe {probe:?} mapped wrong");
        }
    }

    #[test]
    fn non_virtualization_states_ignore_the_vm_flag() {
        for vm in [true, false] {
            assert_eq!(
                status_user_message_for(DockerStatus::DaemonStopped, vm),
                status_user_message_for(DockerStatus::DaemonStopped, !vm)
            );
            assert_eq!(
                status_user_message_for(DockerStatus::NotInstalled, vm),
                status_user_message_for(DockerStatus::NotInstalled, !vm)
            );
        }
    }
}

/// B19 — "Docker unavailable" while Docker Desktop is running.
///
/// Every docker spawn site resolved `docker` by bare name against the PATH
/// THIS PROCESS was launched with, and Windows does not refresh a running
/// process's environment. Installing Docker, or fixing its PATH entry, while
/// FEM was already running therefore left it permanently CliMissing, and the
/// only cure was restarting the app.
#[cfg(test)]
mod b19_docker_cli_resolution_tests {
    use super::*;

    #[test]
    fn falls_back_to_a_known_install_path_when_the_launch_time_path_misses() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = tmp.path().join("docker.exe");
        std::fs::write(&cli, b"stub").unwrap();

        assert_eq!(
            pick_docker_cli(false, std::slice::from_ref(&cli)),
            Some(cli)
        );
    }

    #[test]
    fn keeps_the_bare_name_when_the_cli_is_already_on_path() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = tmp.path().join("docker.exe");
        std::fs::write(&cli, b"stub").unwrap();

        assert_eq!(
            pick_docker_cli(true, &[cli]),
            None,
            "an on-PATH docker must keep resolving by name"
        );
    }

    #[test]
    fn ignores_candidates_that_do_not_exist() {
        let tmp = tempfile::tempdir().unwrap();

        assert_eq!(pick_docker_cli(false, &[tmp.path().join("nope.exe")]), None);
        assert_eq!(pick_docker_cli(false, &[]), None);
    }

    #[test]
    fn the_first_existing_candidate_wins() {
        let tmp = tempfile::tempdir().unwrap();
        let first = tmp.path().join("first.exe");
        let second = tmp.path().join("second.exe");
        std::fs::write(&second, b"stub").unwrap();

        assert_eq!(
            pick_docker_cli(false, &[first, second.clone()]),
            Some(second),
            "a missing earlier candidate must not stop the search"
        );
    }

    /// The CLI lives under the Docker Desktop install, not beside it.
    #[test]
    fn the_cli_is_derived_from_the_desktop_install_path() {
        let derived = cli_beside_desktop_exe(std::path::Path::new(
            "C:/Program Files/Docker/Docker/Docker Desktop.exe",
        ))
        .expect("a rooted path has a parent");

        assert!(derived.ends_with("resources/bin/docker.exe"), "{derived:?}");
        assert!(cli_beside_desktop_exe(std::path::Path::new("")).is_none());
    }
}

/// B19 — hard install evidence must outrank a fail-open firmware probe.
#[cfg(test)]
mod b19_docker_status_precedence_tests {
    use super::*;

    #[test]
    fn install_evidence_outranks_the_firmware_probe_when_the_cli_is_missing() {
        assert_eq!(
            resolve_docker_status(DockerProbe::CliMissing, true, false),
            DockerStatus::DaemonStopped,
            "a machine with Docker demonstrably installed must not read as \
             'Docker unavailable' because a CIM query said otherwise"
        );
    }

    #[test]
    fn a_genuinely_absent_install_with_a_negative_probe_is_still_virtualization_disabled() {
        assert_eq!(
            resolve_docker_status(DockerProbe::CliMissing, false, false),
            DockerStatus::VirtualizationDisabled
        );
    }

    #[test]
    fn an_unreachable_daemon_with_a_negative_probe_is_unchanged() {
        assert_eq!(
            resolve_docker_status(DockerProbe::DaemonUnreachable, true, false),
            DockerStatus::VirtualizationDisabled,
            "the other arm must not have moved"
        );
    }

    #[test]
    fn an_absent_install_with_a_healthy_probe_is_simply_not_installed() {
        assert_eq!(
            resolve_docker_status(DockerProbe::CliMissing, false, true),
            DockerStatus::NotInstalled
        );
    }
}

/// B19 — a probe reading must not be trusted for the life of the process.
#[cfg(test)]
mod b19_probe_cache_ttl_tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn a_reading_inside_the_ttl_is_reused() {
        let now = Instant::now();
        assert!(cache_is_fresh(
            Some(now),
            now + Duration::from_secs(599),
            VIRT_CACHE_TTL
        ));
    }

    #[test]
    fn a_reading_past_the_ttl_is_re_probed() {
        let now = Instant::now();
        assert!(!cache_is_fresh(
            Some(now),
            now + Duration::from_secs(601),
            VIRT_CACHE_TTL
        ));
    }

    #[test]
    fn an_empty_cache_is_never_fresh() {
        assert!(!cache_is_fresh(None, Instant::now(), VIRT_CACHE_TTL));
    }

    /// The range check below only pins the constant. This pins that
    /// `virtualization_supported` actually CONSULTS the TTL cache and no longer
    /// caches for the whole process lifetime.
    #[test]
    fn the_virtualization_probe_consults_the_ttl_cache_not_a_process_lifetime_oncelock() {
        let src = include_str!("docker_manager.rs");
        let at = src
            .find(&format!("pub fn virtualization{}()", "_supported"))
            .expect("virtualization_supported must exist");
        let end = src[at..]
            .find(&format!("\nfn probe_virtualization{}()", "_supported"))
            .map(|e| at + e)
            .expect("the probe body must follow the cached accessor");
        let body = &src[at..end];

        // Comments are stripped before the OnceLock check: the doc comment in
        // this region deliberately NAMES the OnceLock it replaced, and a guard
        // that trips over its own explanation is worse than no guard.
        let code: String = body
            .lines()
            .map(|l| l.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            code.contains(&format!("cache_is{}(", "_fresh")),
            "the probe does not consult the TTL cache:\n{code}"
        );
        assert!(
            !code.contains("OnceLock"),
            "the probe still caches its reading for the whole process lifetime:\n{code}"
        );
    }

    /// A reading taken before WSL2 had finished coming up must not outlive the
    /// condition it measured.
    #[test]
    fn the_ttl_is_bounded_in_minutes_not_in_process_lifetime() {
        assert!(VIRT_CACHE_TTL <= Duration::from_secs(900));
        assert!(VIRT_CACHE_TTL >= Duration::from_secs(60));
    }
}

/// FAIL-14: the CLI-spawn-failure WARN fired on every probe. Separate file so
/// the inline `mod tests`/`mod b19_*` blocks above stay byte-identical.
#[cfg(test)]
#[path = "docker_manager_warn_once_tests.rs"]
mod docker_manager_warn_once_tests;

/// c4 BUG LOOP 4 (BL4-A): the installer download needs the caller's gesture.
#[cfg(test)]
#[path = "docker_gesture_download_tests.rs"]
mod docker_gesture_download_tests;

/// c4 BUG LOOP 5 (lens-1 NB): the caller's trigger reaches the core.
#[cfg(test)]
#[path = "docker_trigger_passthrough_tests.rs"]
mod docker_trigger_passthrough_tests;
