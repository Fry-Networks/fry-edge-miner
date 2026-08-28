use crate::supervisor::platform::BoundedOutput;
use super::download::{download_file_with_options, partners_base_dir};
use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{info, warn};

const SPACE_ACRES_MIN_GB: u64 = 50;

/// The release artifact chosen for this platform.
pub struct ReleaseAsset {
    pub version: String,
    pub asset_name: String,
    pub download_url: String,
}

pub struct SpaceAcresIntegration;

const GITHUB_API_URL: &str = "https://api.github.com/repos/autonomys/space-acres/releases/latest";
const USER_AGENT: &str = concat!("FryEdgeMiner/", env!("CARGO_PKG_VERSION"));

/// Every location the Space Acres binary can live under the discovery roots.
/// Upstream's WiX package places it in a `bin` subdirectory
/// (`<root>\Space Acres\bin\space-acres.exe`); older portable layouts keep it
/// at the root. Pure so the layout list is testable.
/// Every install root Space Acres can occupy under one base directory.
/// Upstream's WiX per-user install lands in `<base>\Programs\Space Acres`
/// (capitalised, matching its product name) while per-machine installs use
/// `<base>\Space Acres`; the lowercase spellings cover older portable
/// layouts. Pure so the layout list is testable.
fn roots_for_base(base: &std::path::Path) -> Vec<PathBuf> {
    vec![
        base.join("space-acres"),
        base.join("Programs").join("space-acres"),
        base.join("Space Acres"),
        base.join("Programs").join("Space Acres"),
    ]
}

fn binary_candidates(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots
        .iter()
        .flat_map(|root| {
            [
                root.join("space-acres.exe"),
                root.join("bin").join("space-acres.exe"),
            ]
        })
        .collect()
}

/// Whether install() actually needs to run the upstream installer. A live
/// farmer process is proof of install even when path discovery misses —
/// re-running the Burn installer over an existing install is what showed the
/// Modify/Repair maintenance dialog on every launch.
fn install_needed(running: bool, binary_found: bool) -> bool {
    !running && !binary_found
}

/// The PE section name WiX Burn stamps into every bootstrapper it builds.
const BURN_SECTION_MARKER: &[u8] = b".wixburn";

/// Whether a PE header carries the `.wixburn` section, i.e. the file is a WiX
/// Burn bootstrapper (upstream's Windows *installer*) rather than the farmer.
/// Pure so it is testable without touching the filesystem.
fn head_is_burn_bundle(head: &[u8]) -> bool {
    head.windows(BURN_SECTION_MARKER.len())
        .any(|w| w == BURN_SECTION_MARKER)
}

/// Section headers sit within the first few KB of a PE image.
#[cfg(target_os = "windows")]
fn file_is_burn_bundle(path: &std::path::Path) -> bool {
    use std::io::Read;
    let mut buf = [0u8; 8192];
    match std::fs::File::open(path) {
        Ok(mut f) => match f.read(&mut buf) {
            Ok(n) => head_is_burn_bundle(&buf[..n]),
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// Pick the binary to launch, given the staged copy and whatever path discovery
/// found. FEM <= 0.4.20 saved the downloaded Burn bootstrapper as
/// `space-acres.exe` in its own partner directory and then preferred that copy
/// unconditionally, so `start()` spawned the INSTALLER on every launch and the
/// user got the "Modify Setup" maintenance dialog instead of a farmer. The
/// staged copy is only usable when it is the farmer itself.
fn pick_binary(
    staged: Option<PathBuf>,
    staged_is_installer: bool,
    discovered: Option<PathBuf>,
) -> Option<PathBuf> {
    staged.filter(|_| !staged_is_installer).or(discovered)
}

impl SpaceAcresIntegration {
    fn partner_dir() -> PathBuf {
        partners_base_dir().join("space_acres")
    }

    fn binary_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        return Self::partner_dir().join("space-acres.exe");
        #[cfg(not(target_os = "windows"))]
        return Self::partner_dir().join("space-acres");
    }

    /// Where the downloaded Windows release artifact is staged.
    ///
    /// Upstream ships Windows as an INSTALLER, not a portable binary: the
    /// `space-acres-<ver>-x86_64.exe` asset is a WiX Burn bootstrapper (three
    /// `.wixburn` PE sections; contains none of the app's own strings), and
    /// upstream's INSTALLATION.md tells Windows users to run an installer.
    /// Saving it as `space-acres.exe` and executing it with `--base-directory`
    /// launched the installer UI instead of a farmer — the "SpaceAcres does not
    /// download properly on Windows" report.
    #[cfg(target_os = "windows")]
    fn installer_path(asset_name: &str) -> PathBuf {
        let ext = if asset_name.to_lowercase().ends_with(".msi") {
            "msi"
        } else {
            "exe"
        };
        Self::partner_dir().join(format!("space-acres-installer.{ext}"))
    }

    /// Locate the binary the installer actually placed, mirroring the Olostep
    /// integration's post-install discovery. Returns the first hit.
    #[cfg(target_os = "windows")]
    fn installed_binary() -> Option<PathBuf> {
        // A previously-staged portable copy still wins, so existing installs
        // that already work keep working — but only when it really is the
        // farmer. Builds up to 0.4.20 staged the Burn bootstrapper under this
        // name, and preferring that meant every start() ran the installer.
        let staged = Self::binary_path();
        let staged_exists = staged.exists();
        let staged_is_installer = staged_exists && file_is_burn_bundle(&staged);
        let staged = staged_exists.then_some(staged);
        let mut roots: Vec<PathBuf> = Vec::new();
        for var in ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)", "ProgramData"] {
            if let Ok(base) = std::env::var(var) {
                roots.extend(roots_for_base(std::path::Path::new(&base)));
            }
        }
        let discovered = binary_candidates(&roots).into_iter().find(|c| c.exists());
        pick_binary(staged, staged_is_installer, discovered)
    }

    /// Move a mis-staged Burn bootstrapper out of the farmer's filename so it
    /// stops being launched. Renamed rather than deleted: the file is a valid
    /// installer, and a rename is trivially reversible.
    #[cfg(target_os = "windows")]
    fn quarantine_staged_installer() {
        let staged = Self::binary_path();
        if !staged.exists() || !file_is_burn_bundle(&staged) {
            return;
        }
        let dest = Self::partner_dir().join("space-acres-installer.exe");
        match std::fs::rename(&staged, &dest) {
            Ok(()) => info!(from = ?staged, to = ?dest, "Quarantined mis-staged SpaceAcres installer"),
            Err(e) => warn!(error = %e, path = ?staged, "Could not quarantine mis-staged SpaceAcres installer"),
        }
    }

    fn github_token() -> Option<String> {
        std::env::var("GITHUB_TOKEN").ok().filter(|s| !s.is_empty())
    }

    fn build_client() -> reqwest::Client {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(token) = Self::github_token() {
            headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token))
                    .expect("invalid GITHUB_TOKEN header value"),
            );
        }

        reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .user_agent(USER_AGENT)
            .default_headers(headers)
            .build()
            .expect("failed to build GitHub HTTP client")
    }

    async fn fetch_latest_release() -> Result<ReleaseAsset> {
        let client = Self::build_client();
        let max_attempts = 3u32;
        let base_delay = Duration::from_secs(2);
        let mut last_error = None;

        for attempt in 1..=max_attempts {
            info!(url = GITHUB_API_URL, attempt = attempt, "Fetching latest SpaceAcres release");

            match client.get(GITHUB_API_URL).send().await {
                Ok(response) => {
                    let status = response.status();

                    if status.is_success() {
                        let json: serde_json::Value = response.json().await?;
                        let tag_name = json["tag_name"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("No tag_name in release"))?
                            .to_string();

                        let assets = json["assets"]
                            .as_array()
                            .ok_or_else(|| anyhow::anyhow!("No assets in release"))?;

                        // On Windows prefer a plain `.msi` when upstream publishes
                        // one (their INSTALLATION.md documents the .msi path);
                        // fall back to the Burn `.exe` bundle otherwise.
                        let platform_suffixes: &[&str] = if cfg!(target_os = "windows") {
                            &[".msi", ".exe"]
                        } else if cfg!(target_os = "macos") {
                            &[".dmg"]
                        } else {
                            &[".AppImage"]
                        };

                        let host_arch = std::env::consts::ARCH; // "x86_64", "aarch64", etc.

                        let picked = platform_suffixes.iter().find_map(|suffix| {
                            assets.iter().find_map(|asset| {
                                let name = asset["name"].as_str()?;
                                if name.ends_with(suffix) && name.contains(host_arch) {
                                    let url = asset["browser_download_url"].as_str()?;
                                    return Some((name.to_string(), url.to_string()));
                                }
                                None
                            })
                        });

                        let (asset_name, download_url) = picked.ok_or_else(|| {
                            anyhow::anyhow!(
                                "No {:?} asset found for arch {} in release",
                                platform_suffixes,
                                host_arch
                            )
                        })?;

                        return Ok(ReleaseAsset {
                            version: tag_name,
                            asset_name,
                            download_url,
                        });
                    }

                    let headers = response.headers();
                    let ratelimit_remaining = headers
                        .get("x-ratelimit-remaining")
                        .and_then(|v| v.to_str().ok());
                    let retry_after = headers
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok());

                    warn!(
                        url = GITHUB_API_URL,
                        status = status.as_u16(),
                        ratelimit_remaining = ?ratelimit_remaining,
                        retry_after = ?retry_after,
                        attempt = attempt,
                        "Failed to fetch latest SpaceAcres release"
                    );

                    if status == reqwest::StatusCode::FORBIDDEN
                        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
                    {
                        last_error = Some(anyhow::anyhow!(
                            "GitHub API returned HTTP {} (x-ratelimit-remaining={:?}, retry-after={:?})",
                            status.as_u16(),
                            ratelimit_remaining,
                            retry_after
                        ));

                        if attempt < max_attempts {
                            let delay = base_delay * 2u32.pow(attempt - 1);
                            warn!(delay = ?delay, "Retrying GitHub API call after rate-limit backoff");
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                    } else {
                        return Err(anyhow::anyhow!("Failed to fetch latest release: HTTP {}", status.as_u16()));
                    }
                }
                Err(e) => {
                    warn!(error = %e, attempt = attempt, "GitHub API request error");
                    last_error = Some(anyhow::anyhow!("GitHub API request error: {}", e));
                    if attempt < max_attempts {
                        let delay = base_delay * 2u32.pow(attempt - 1);
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Failed to fetch latest release after all retries")))
    }

    fn is_running() -> bool {
        #[cfg(target_os = "windows")]
        {
            crate::supervisor::platform::command("tasklist")
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .to_lowercase()
                        .contains("space-acres.exe")
                })
                .unwrap_or(false)
        }
        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    }

    /// Check if system meets SpaceAcres eligibility requirements:
    /// - System has an SSD (or the SeekPenalty=false fallback indicates one)
    /// - Free disk space >= SPACE_ACRES_MIN_GB
    /// Returns (eligible, Option<reason>) — if ineligible, reason explains why.
    pub async fn check_eligibility() -> (bool, Option<String>) {
        if !has_ssd() {
            return (false, Some("No SSD detected — SpaceAcres requires solid-state storage".to_string()));
        }

        // Check free disk space on the partners base directory
        match check_free_space().await {
            Ok(free_gb) => {
                if free_gb < SPACE_ACRES_MIN_GB {
                    return (
                        false,
                        Some(format!(
                            "Insufficient disk space — {} GB free, {} GB required",
                            free_gb, SPACE_ACRES_MIN_GB
                        )),
                    );
                }
                (true, None)
            }
            Err(e) => {
                warn!(error = %e, "Failed to check disk space");
                // TODO-verify: SpaceAcres actual minimum
                // Assume eligible if check fails (don't block on uncertain state)
                (true, None)
            }
        }
    }
}

#[async_trait]
impl Integration for SpaceAcresIntegration {
    fn id(&self) -> &str {
        "space_acres"
    }

    fn display_name(&self) -> &str {
        "SpaceAcres"
    }

    async fn install(&self) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            Self::quarantine_staged_installer();
            let binary_found = match Self::installed_binary() {
                Some(existing) => {
                    info!(path = ?existing, "SpaceAcres already installed");
                    true
                }
                None => false,
            };
            if !install_needed(Self::is_running(), binary_found) {
                if !binary_found {
                    info!("SpaceAcres process already running — treating as installed");
                }
                return Ok(());
            }

            info!("Installing SpaceAcres from GitHub latest release");
            let release = Self::fetch_latest_release().await?;
            info!(
                version = %release.version,
                asset = %release.asset_name,
                "Found latest release"
            );

            let installer = Self::installer_path(&release.asset_name);
            std::fs::create_dir_all(Self::partner_dir())?;
            let token = Self::github_token();
            download_file_with_options(
                &release.download_url,
                &installer,
                USER_AGENT,
                token.as_deref(),
            )
            .await?;

            // The Windows artifact is an installer, so RUN it silently rather
            // than treating it as the farmer binary.
            let is_msi = release.asset_name.to_lowercase().ends_with(".msi");
            let output = if is_msi {
                crate::supervisor::platform::command("msiexec")
                    .arg("/i")
                    .arg(&installer)
                    .args(["/quiet", "/norestart"])
                    .output_bounded(crate::supervisor::platform::LONG_TIMEOUT)?
            } else {
                // WiX Burn bootstrapper flags.
                crate::supervisor::platform::command(&installer)
                    .args(["/quiet", "/norestart"])
                    .output_bounded(crate::supervisor::platform::LONG_TIMEOUT)?
            };
            if !output.status.success() {
                warn!(
                    code = output.status.code(),
                    "SpaceAcres installer exited non-zero (it may still be finishing)"
                );
            }

            // Installers finish asynchronously — wait for the real binary.
            let mut found = None;
            for _ in 0..60 {
                if let Some(p) = Self::installed_binary() {
                    found = Some(p);
                    break;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            let binary = found.ok_or_else(|| {
                anyhow::anyhow!(
                    "SpaceAcres installer ran but space-acres.exe did not appear within 120 seconds{}. Antivirus or an elevation prompt may have blocked it.",
                    output
                        .status
                        .code()
                        .map(|c| format!(" (installer exit code {c})"))
                        .unwrap_or_default()
                )
            })?;

            if let Err(e) = std::fs::remove_file(&installer) {
                warn!(error = %e, "Could not remove SpaceAcres installer");
            }
            info!(binary = ?binary, version = %release.version, "SpaceAcres installed successfully");
            return Ok(());
        }

        #[cfg(not(target_os = "windows"))]
        {
            let binary = Self::binary_path();
            if binary.exists() {
                info!(path = ?binary, "SpaceAcres binary already installed");
                return Ok(());
            }

            info!("Installing SpaceAcres from GitHub latest release");
            let release = Self::fetch_latest_release().await?;
            info!(version = %release.version, asset = %release.asset_name, "Found latest release");

            let token = Self::github_token();
            download_file_with_options(
                &release.download_url,
                &binary,
                USER_AGENT,
                token.as_deref(),
            )
            .await?;

            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o755);
            std::fs::set_permissions(&binary, perms)?;

            info!(binary = ?binary, version = %release.version, "SpaceAcres installed successfully");
            Ok(())
        }
    }

    async fn start(&self) -> Result<()> {
        #[cfg(target_os = "windows")]
        let binary = Self::installed_binary()
            .ok_or_else(|| anyhow::anyhow!("SpaceAcres is not installed — enable it to install"))?;
        #[cfg(not(target_os = "windows"))]
        let binary = Self::binary_path();

        if !binary.exists() {
            anyhow::bail!("SpaceAcres binary not found at {:?}", binary);
        }

        if Self::is_running() {
            info!("SpaceAcres already running");
            return Ok(());
        }

        info!(binary = ?binary, "Starting SpaceAcres");

        // Spawn the process with a base directory argument
        let base_dir = Self::partner_dir().join("data");
        std::fs::create_dir_all(&base_dir)?;

        let _ = crate::supervisor::platform::command(&binary)
            .arg("--base-directory")
            .arg(&base_dir)
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!("Failed to start SpaceAcres: {}", e)
            })?;

        // Give it a moment to start
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        // Kill any running space-acres process
        #[cfg(target_os = "windows")]
        {
            let _ = crate::supervisor::platform::command("taskkill")
                .args(["/IM", "space-acres.exe", "/F"])
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT);
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = crate::supervisor::platform::command("killall")
                .arg("space-acres")
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT);
        }

        info!("Stopped SpaceAcres");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        // F6: resolve the real install location (Program Files on Windows), so a
        // running farmer reports Healthy instead of a permanent Stopped.
        #[cfg(target_os = "windows")]
        let installed = Self::installed_binary().is_some();
        #[cfg(not(target_os = "windows"))]
        let installed = Self::binary_path().exists();
        if !installed {
            return HealthStatus::Stopped;
        }
        if !has_ssd() {
            warn!("No SSD detected — SpaceAcres performance will be degraded");
            return HealthStatus::Unhealthy(
                "No SSD detected — SpaceAcres performance degraded".to_string(),
            );
        }
        if Self::is_running() {
            HealthStatus::Healthy
        } else {
            HealthStatus::Stopped
        }
    }

    async fn check_update(&self) -> Result<Option<String>> {
        match Self::fetch_latest_release().await {
            Ok(release) => {
                info!(version = %release.version, "Found SpaceAcres update available");
                Ok(Some(release.version))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to check SpaceAcres updates");
                Ok(None)
            }
        }
    }

    async fn apply_update(&self, version: &str) -> Result<()> {
        info!(version = %version, "Applying SpaceAcres update");
        // Stop the current instance
        self.stop().await?;
        // Backup old binary
        let binary = Self::binary_path();
        if binary.exists() {
            let backup = binary.with_extension("exe.bak");
            std::fs::copy(&binary, &backup)?;
        }
        // Re-run install which will download the latest
        self.install().await?;
        Ok(())
    }

    fn installed_version(&self) -> Option<String> {
        // F6: on Windows the installer drops space-acres.exe into Program Files,
        // not our staged partner dir — so resolve the same way start() does, or
        // main.rs treats an installed farmer as "not installed" and re-runs the
        // installer on every launch.
        #[cfg(target_os = "windows")]
        let present = Self::installed_binary().is_some();
        #[cfg(not(target_os = "windows"))]
        let present = Self::binary_path().exists();
        if present {
            Some("installed".into())
        } else {
            None
        }
    }

    fn collect_poc_data(&self) -> PocGateData {
        PocGateData {
            poa: Self::is_running(),
            ..Default::default()
        }
    }
}

/// Check available disk space on the partners directory.
/// Returns available space in GB.
async fn check_free_space() -> anyhow::Result<u64> {
    let base_dir = partners_base_dir();
    #[cfg(target_os = "windows")]
    {
        let path = base_dir.to_string_lossy().to_string();
        let drive = path.split(':').next().unwrap_or("C");

        let output = crate::supervisor::platform::command("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "((Get-Volume -DriveLetter {} | Select-Object -Expand SizeRemaining) / 1GB) -as [int64]",
                    drive
                ),
            ])
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)?;

        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u64>()
            .map_err(|e| anyhow::anyhow!("Failed to parse disk space: {}", e))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let output = crate::supervisor::platform::command("df")
            .arg("-BG")
            .arg(&base_dir)
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)?;

        // df output format: Filesystem 1G-blocks Used Available Use% Mounted on
        let lines: Vec<&str> = String::from_utf8_lossy(&output.stdout).lines().collect();
        if lines.len() > 1 {
            let parts: Vec<&str> = lines[1].split_whitespace().collect();
            if parts.len() > 3 {
                return parts[3]
                    .trim_end_matches('G')
                    .parse::<u64>()
                    .map_err(|e| anyhow::anyhow!("Failed to parse disk space: {}", e));
            }
        }
        anyhow::bail!("Failed to parse df output")
    }
}

/// Detect if system has an SSD.
/// Cached: the probe spawns a full PowerShell process (~1-3s) and this is
/// called from the 30s health-check loop — uncached it burns CPU forever,
/// and physical disks don't change while the app runs.
///
/// Primary: Get-PhysicalDisk | Where MediaType -eq 'SSD'
/// Fallback: MSFT_PhysicalDisk with SeekPenalty==false (indicates SSD)
#[cfg(target_os = "windows")]
fn has_ssd() -> bool {
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| {
        // Primary check: explicit SSD MediaType
        let primary_check = crate::supervisor::platform::command("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-PhysicalDisk | Where MediaType -eq 'SSD' | Measure-Object | Select -Expand Count",
            ])
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<u32>()
                    .unwrap_or(0)
                    > 0
            })
            .unwrap_or(false);

        if primary_check {
            return true;
        }

        // Fallback: MSFT_PhysicalDisk with SeekPenalty==false indicates SSD
        crate::supervisor::platform::command("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-WmiObject -Namespace \"root/Microsoft/Windows/Storage\" -Class MSFT_PhysicalDisk | Where-Object SeekPenalty -EQ $false | Measure-Object).Count -gt 0",
            ])
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .to_lowercase()
                    .contains("true")
            })
            .unwrap_or(false)
    })
}

#[cfg(not(target_os = "windows"))]
fn has_ssd() -> bool {
    false // Platform-specific SSD detection deferred
}

/// F6 follow-up: upstream's WiX package (res/windows/wix/space-acres.wxs)
/// installs the farmer to `<root>\Space Acres\bin\space-acres.exe` — a `bin`
/// level the discovery probe missed, so every fresh MSI install read as "not
/// installed" and the Burn installer re-ran (showing its Modify/Repair
/// maintenance dialog) on every launch.
#[cfg(test)]
mod discovery_tests {
    use super::*;

    #[test]
    fn the_wix_bin_subdirectory_is_probed() {
        let roots = vec![PathBuf::from(r"C:\Program Files\Space Acres")];
        let candidates = binary_candidates(&roots);
        assert!(
            candidates.contains(&PathBuf::from(r"C:\Program Files\Space Acres\bin\space-acres.exe")),
            "candidates must include the WiX bin layout: {candidates:?}"
        );
        assert!(
            candidates.contains(&PathBuf::from(r"C:\Program Files\Space Acres\space-acres.exe")),
            "the flat layout must keep working: {candidates:?}"
        );
    }

    #[test]
    fn the_capitalised_per_user_programs_root_is_probed() {
        // WiX per-user installs land in %LOCALAPPDATA%\Programs\Space Acres,
        // using the product's capitalised name. That root was missing, so a
        // per-user install read as "not installed" and the startup recovery
        // re-ran the installer — the Modify/Repair dialog users reported.
        let roots = roots_for_base(std::path::Path::new(r"C:\Users\x\AppData\Local"));
        assert!(
            roots.contains(&PathBuf::from(r"C:\Users\x\AppData\Local\Programs\Space Acres")),
            "per-user capitalised root must be probed: {roots:?}"
        );
        let candidates = binary_candidates(&roots);
        assert!(
            candidates.contains(&PathBuf::from(
                r"C:\Users\x\AppData\Local\Programs\Space Acres\bin\space-acres.exe"
            )),
            "and its WiX bin layout must resolve: {candidates:?}"
        );
    }

    #[test]
    fn every_previously_probed_root_still_resolves() {
        let roots = roots_for_base(std::path::Path::new(r"C:\Program Files"));
        for expected in [
            r"C:\Program Files\space-acres",
            r"C:\Program Files\Programs\space-acres",
            r"C:\Program Files\Space Acres",
        ] {
            assert!(
                roots.contains(&PathBuf::from(expected)),
                "{expected} must still be probed: {roots:?}"
            );
        }
    }

    #[test]
    fn a_running_farmer_never_triggers_a_reinstall() {
        // The installer re-run is exactly the Repair-dialog loop, so a live
        // process is proof of install even when path discovery misses.
        assert!(!install_needed(true, false));
        assert!(!install_needed(false, true));
        assert!(install_needed(false, false));
    }

    /// A real staged artifact from a machine showing the repair loop had
    /// `OriginalFilename: space-acres-0.2.21-x86_64.exe` and a `.wixburn`
    /// section: it was the installer saved under the farmer's name.
    #[test]
    fn burn_bundle_is_detected_from_pe_section_names() {
        let mut head = b"MZ\x90\x00.text\x00\x00\x00.rdata\x00\x00".to_vec();
        head.extend_from_slice(b".wixburn8");
        head.extend_from_slice(b".rsrc\x00\x00\x00");
        assert!(head_is_burn_bundle(&head));
    }

    #[test]
    fn farmer_binary_is_not_mistaken_for_a_burn_bundle() {
        let head = b"MZ\x90\x00.text\x00\x00\x00.rdata\x00\x00.data\x00\x00\x00.rsrc\x00\x00\x00".to_vec();
        assert!(!head_is_burn_bundle(&head));
    }

    #[test]
    fn staged_installer_is_skipped_in_favour_of_the_real_install() {
        let staged = PathBuf::from(r"C:\Users\u\AppData\Roaming\FryEdgeMiner\partners\space_acres\space-acres.exe");
        let discovered = PathBuf::from(r"C:\Program Files\Space Acres\bin\space-acres.exe");
        // Staged copy is really the Burn installer: launching it shows the
        // "Modify Setup" dialog, so the installed farmer must win instead.
        assert_eq!(
            pick_binary(Some(staged), true, Some(discovered.clone())),
            Some(discovered)
        );
    }

    #[test]
    fn staged_farmer_still_wins_when_it_is_a_real_binary() {
        let staged = PathBuf::from(r"C:\staged\space-acres.exe");
        let discovered = PathBuf::from(r"C:\Program Files\Space Acres\bin\space-acres.exe");
        assert_eq!(
            pick_binary(Some(staged.clone()), false, Some(discovered)),
            Some(staged)
        );
    }

    #[test]
    fn staged_installer_with_no_install_found_yields_nothing_to_launch() {
        let staged = PathBuf::from(r"C:\staged\space-acres.exe");
        assert_eq!(pick_binary(Some(staged), true, None), None);
    }
}
