use super::download::{download_file_with_options, partners_base_dir};
use super::{tracked_child_probe, HealthStatus, Integration, PocGateData};
use crate::supervisor::platform::BoundedOutput;
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tracing::{info, warn};

const SPACE_ACRES_MIN_GB: u64 = 50;

/// Whether this machine meets SpaceAcres' published minimums, from already
/// memoised probe results. Pure so the thresholds are testable without
/// shelling out to PowerShell — same split Iagon uses.
///
/// Fails OPEN on `None`: an unmeasurable machine must never be silently marked
/// unavailable, because `check_requirements()` drives `available_count()`,
/// which is the denominator of the multiplier this device submits.
/// PURE: the `Result` -> `(eligible, reason)` shape the toggle expects. Split
/// out so the mapping is testable without a disk.
fn eligibility_from(verdict: Result<(), String>) -> (bool, Option<String>) {
    match verdict {
        Ok(()) => (true, None),
        Err(r) => (false, Some(r)),
    }
}

fn evaluate_requirements(ssd: Option<bool>, free_gb: Option<f64>) -> Result<(), String> {
    if ssd == Some(false) {
        return Err("No SSD detected — SpaceAcres requires solid-state storage".to_string());
    }
    if let Some(gb) = free_gb {
        if gb < SPACE_ACRES_MIN_GB as f64 {
            return Err(format!(
                "Insufficient disk space — SpaceAcres needs {} GB free, this device has {:.0} GB available",
                SPACE_ACRES_MIN_GB, gb
            ));
        }
    }
    Ok(())
}

/// The release artifact chosen for this platform.
pub struct ReleaseAsset {
    pub version: String,
    pub asset_name: String,
    pub download_url: String,
}

/// BUG 3: SpaceAcres was spawned via `.spawn()` with the returned `Child`
/// immediately dropped (`let _ = ...spawn()...`), so FEM had no way to tell
/// ITS OWN spawned process apart from an adopted/pre-existing one — every
/// liveness check, `stop()`, and restart went through an untargeted
/// `tasklist`/`taskkill /IM` image-name scan. Tracking the `Child` gives a
/// fast, reliable, no-shell-out liveness check for the common case (FEM
/// started it) while the image-name probe remains the fallback for adoption
/// (an instance FEM did not start, e.g. one Windows autostarted at login).
#[derive(Default)]
pub struct SpaceAcresIntegration {
    child: Mutex<Option<std::process::Child>>,
}

/// Whether the version string read off a staged partners-dir copy signals it
/// is stale relative to the version the OS-managed (Program Files/LocalAppData)
/// install reports. Pure — simple inequality, not a semver comparison (the
/// upstream tag format is not guaranteed semver-clean), matching the BUG 3
/// requirement to quarantine a staged copy "whose version differs from the
/// Program Files farmer". `None` on either side means "can't tell" and never
/// triggers quarantine — an unmeasurable version must not evict a working
/// staged farmer (same fail-open principle as `evaluate_requirements`).
fn staged_is_stale(staged_version: Option<&str>, discovered_version: Option<&str>) -> bool {
    match (staged_version, discovered_version) {
        (Some(s), Some(d)) => s != d,
        _ => false,
    }
}

/// BUG 7: parse a loose version string ("v0.2.21", "0.2.21.0") into numeric
/// components for ordered comparison. A non-numeric/malformed component
/// reads as 0 rather than failing the whole parse — best-effort, matching
/// this codebase's fail-open-on-uncertain-data style elsewhere.
fn parse_version_components(v: &str) -> Vec<u32> {
    v.trim_start_matches(|c: char| !c.is_ascii_digit())
        .split('.')
        .map(|p| p.parse::<u32>().unwrap_or(0))
        .collect()
}

/// Whether `latest` (a GitHub release tag, e.g. "v0.2.21") is genuinely
/// newer than `installed` (a PE ProductVersion resource, e.g. "0.2.21.0") —
/// compares numeric components pairwise, zero-padded to the longer length,
/// so a trailing ".0" the version resource adds never causes a false
/// "update available". `None` installed (unmeasurable) fails OPEN toward
/// offering the update — an offer is reversible; silently withholding one
/// is not, and matches `check_update`'s prior always-available behavior for
/// that specific case.
fn update_available(installed: Option<&str>, latest: &str) -> bool {
    let Some(installed) = installed else {
        return true;
    };
    let mut a = parse_version_components(installed);
    let mut b = parse_version_components(latest);
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    b > a
}

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

/// The `MZ` DOS-header magic every valid PE image (installer or farmer)
/// starts with. A staged file that lacks it is not "the installer instead of
/// the farmer" (that's `head_is_burn_bundle`'s job) — it is corrupted:
/// truncated, bit-flipped, or garbage from a partial/interrupted write (disk
/// full, power loss mid-write, antivirus quarantine-and-restore). Pure, same
/// idiom as `head_is_burn_bundle`, so it is testable without touching the
/// filesystem.
fn head_is_valid_pe(head: &[u8]) -> bool {
    head.len() >= 2 && &head[0..2] == b"MZ"
}

/// File-level counterpart to `head_is_valid_pe`, mirroring
/// `file_is_burn_bundle` exactly (same read pattern, same failure-to-false).
#[cfg(target_os = "windows")]
fn file_is_valid_pe(path: &std::path::Path) -> bool {
    use std::io::Read;
    let mut buf = [0u8; 8192];
    match std::fs::File::open(path) {
        Ok(mut f) => {
            let file_len = match f.metadata() {
                Ok(m) => m.len(),
                Err(_) => return false,
            };
            match f.read(&mut buf) {
                Ok(n) => head_is_valid_pe(&buf[..n]) && pe_head_fits_file(&buf[..n], file_len),
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

/// Bug 4 follow-up (WP3 live trial, 2026-09-07): `head_is_valid_pe` only proves
/// the 2-byte MZ magic, which a farmer binary TRUNCATED mid-write (disk full,
/// power loss, AV quarantine-and-restore) still carries — so the truncated
/// staged copy passed the gate and was spawned in preference to the real
/// install. Walk the PE headers that fit in the first 8 KiB and require every
/// section's raw data to lie inside the file. Anything malformed or cut short
/// is not trusted. Overlay bytes past the last section are not covered.
#[cfg(target_os = "windows")]
fn pe_head_fits_file(head: &[u8], file_len: u64) -> bool {
    let u16_at = |o: usize| head.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |o: usize| {
        head.get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let Some(e_lfanew) = u32_at(0x3C).map(|v| v as usize) else {
        return false;
    };
    if head.get(e_lfanew..e_lfanew + 4) != Some(&b"PE\0\0"[..]) {
        return false;
    }
    let (Some(sections), Some(opt_size)) = (u16_at(e_lfanew + 6), u16_at(e_lfanew + 20)) else {
        return false;
    };
    let table = e_lfanew + 24 + opt_size as usize;
    (0..sections as usize).all(|i| {
        let s = table + i * 40;
        match (u32_at(s + 16), u32_at(s + 20)) {
            (Some(raw_size), Some(raw_ptr)) => raw_ptr as u64 + raw_size as u64 <= file_len,
            _ => false,
        }
    })
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
        // Bug 4: a staged file that is neither the farmer nor a recognised
        // Burn bundle is not "safe by elimination" — it may be a corrupted
        // partial write. Trusting and spawning it unvalidated surfaced as a
        // raw Windows "Unsupported 16-Bit Application" dialog instead of any
        // FEM-owned error, and — worse — it was PREFERRED over a known-good
        // discovered Program Files install. Route it through the same
        // "don't trust the staged copy" branch `pick_binary` already has.
        let staged_is_corrupt = staged_exists && !staged_is_installer && !file_is_valid_pe(&staged);
        let mut roots: Vec<PathBuf> = Vec::new();
        for var in [
            "LOCALAPPDATA",
            "ProgramFiles",
            "ProgramFiles(x86)",
            "ProgramData",
        ] {
            if let Ok(base) = std::env::var(var) {
                roots.extend(roots_for_base(std::path::Path::new(&base)));
            }
        }
        let discovered = binary_candidates(&roots).into_iter().find(|c| c.exists());
        // BUG 3: a staged copy that is neither the installer nor corrupt can
        // still be STALE relative to the OS-managed install — the version
        // the Program Files/LocalAppData farmer actually reports. Best-effort
        // (an unmeasurable version never evicts a working staged farmer —
        // `staged_is_stale`'s fail-open contract).
        let staged_is_stale_copy = staged_exists
            && !staged_is_installer
            && !staged_is_corrupt
            && discovered
                .as_ref()
                .map(|d| {
                    staged_is_stale(
                        Self::file_product_version(&staged).as_deref(),
                        Self::file_product_version(d).as_deref(),
                    )
                })
                .unwrap_or(false);
        let staged_untrusted = staged_is_installer || staged_is_corrupt || staged_is_stale_copy;
        let staged = staged_exists.then_some(staged);
        pick_binary(staged, staged_untrusted, discovered)
    }

    /// Read a file's PE VersionInfo.ProductVersion via PowerShell.
    /// Best-effort: ANY failure (file missing, PowerShell error, no version
    /// resource embedded) reads as `None` — matches `staged_is_stale`'s
    /// fail-open contract (an unmeasurable version must never evict a
    /// working staged farmer).
    #[cfg(target_os = "windows")]
    fn file_product_version(path: &std::path::Path) -> Option<String> {
        let ps_quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
        let out = crate::supervisor::platform::command("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "(Get-Item {}).VersionInfo.ProductVersion",
                    ps_quote(&path.to_string_lossy())
                ),
            ])
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if v.is_empty() {
            None
        } else {
            Some(v)
        }
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
            Ok(()) => {
                info!(from = ?staged, to = ?dest, "Quarantined mis-staged SpaceAcres installer")
            }
            Err(e) => {
                warn!(error = %e, path = ?staged, "Could not quarantine mis-staged SpaceAcres installer")
            }
        }
    }

    /// BUG 3: move a staged copy whose version differs from the discovered
    /// OS-managed farmer out of the way, mirroring
    /// `quarantine_staged_installer`'s rename-not-delete pattern exactly. A
    /// no-op when there is nothing to compare against, the staged copy is
    /// already handled by the installer/corrupt quarantine, or the versions
    /// happen to match.
    #[cfg(target_os = "windows")]
    fn quarantine_stale_staged_copy() {
        let staged = Self::binary_path();
        if !staged.exists() || file_is_burn_bundle(&staged) || !file_is_valid_pe(&staged) {
            return;
        }
        let mut roots: Vec<PathBuf> = Vec::new();
        for var in [
            "LOCALAPPDATA",
            "ProgramFiles",
            "ProgramFiles(x86)",
            "ProgramData",
        ] {
            if let Ok(base) = std::env::var(var) {
                roots.extend(roots_for_base(std::path::Path::new(&base)));
            }
        }
        let Some(discovered) = binary_candidates(&roots).into_iter().find(|c| c.exists()) else {
            return;
        };
        if !staged_is_stale(
            Self::file_product_version(&staged).as_deref(),
            Self::file_product_version(&discovered).as_deref(),
        ) {
            return;
        }
        let dest = Self::partner_dir().join("space-acres-stale.exe");
        match std::fs::rename(&staged, &dest) {
            Ok(()) => {
                info!(from = ?staged, to = ?dest, discovered = ?discovered, "Quarantined stale staged SpaceAcres copy")
            }
            Err(e) => {
                warn!(error = %e, path = ?staged, "Could not quarantine stale staged SpaceAcres copy")
            }
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
            info!(
                url = GITHUB_API_URL,
                attempt = attempt,
                "Fetching latest SpaceAcres release"
            );

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
                    let retry_after = headers.get("retry-after").and_then(|v| v.to_str().ok());

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
                        return Err(anyhow::anyhow!(
                            "Failed to fetch latest release: HTTP {}",
                            status.as_u16()
                        ));
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

        Err(last_error
            .unwrap_or_else(|| anyhow::anyhow!("Failed to fetch latest release after all retries")))
    }

    /// Image-name tasklist probe — fallback for an adopted/untracked
    /// instance (one FEM did not spawn itself, e.g. Windows autostarted it
    /// at login). BUG 3: previously failed OPEN (`.unwrap_or(false)`) — a
    /// tasklist timeout or error read as "not running" and could trigger a
    /// duplicate spawn on top of a farmer that was actually alive. Fails
    /// CLOSED now: assume running when the probe itself could not complete,
    /// the same fail-safe-toward-availability principle already used
    /// elsewhere in this codebase (mysterium/fryvpn dead-process reasons,
    /// `evaluate_requirements`'s fail-open unmeasurable handling).
    fn image_name_probe() -> bool {
        #[cfg(target_os = "windows")]
        {
            crate::supervisor::platform::command("tasklist")
                .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .to_lowercase()
                        .contains("space-acres.exe")
                })
                .unwrap_or(true)
        }
        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    }

    /// Whether SpaceAcres is running. Prefers the tracked child (fast,
    /// exact, no shell-out) and falls back to the image-name probe for an
    /// instance FEM did not spawn itself.
    fn is_running(&self) -> bool {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(running) = tracked_child_probe(&mut guard) {
                return running;
            }
        }
        Self::image_name_probe()
    }

    /// Check if system meets SpaceAcres eligibility requirements:
    ///
    /// - System has an SSD (or the SeekPenalty=false fallback indicates one)
    /// - Free disk space >= SPACE_ACRES_MIN_GB
    ///
    /// Returns (eligible, Option<reason>) — if ineligible, reason explains why.
    /// BUG 1/4 + BUG 7: rebuilt on `evaluate_requirements` so the toggle gate
    /// and `check_requirements()` can no longer disagree.
    ///
    /// Previously this ran its OWN uncached PowerShell probe (`check_free_space`)
    /// that rounded through `u64`, so it could differ from the real gate by up
    /// to 1 GB, and it used `has_ssd()` (`unwrap_or(false)` — fail CLOSED)
    /// while `check_requirements` used the tri-state (fail OPEN). Off-Windows
    /// `has_ssd()` is hardcoded `false`, which made SpaceAcres permanently
    /// untoggleable there while still counting toward `available_count()`.
    ///
    /// Still `async` so the call site in `commands/integration.rs` is untouched.
    pub async fn check_eligibility() -> (bool, Option<String>) {
        eligibility_from(evaluate_requirements(
            ssd_state_for_requirements(),
            crate::system_info::available_disk_gb(&partners_base_dir()),
        ))
    }
}

#[async_trait]
impl Integration for SpaceAcresIntegration {
    fn id(&self) -> &str {
        "space_acres"
    }

    /// SpaceAcres needs an SSD and headroom to plot. Without this the farmer
    /// counted toward `available_count()` on machines that can never run it,
    /// which shrinks every other integration's share of the multiplier this
    /// device submits — i.e. under-provisioned devices were paid less than
    /// they earned. Reads only memoised probes, per the trait's cheapness rule.
    fn check_requirements(&self) -> Result<(), String> {
        evaluate_requirements(
            ssd_state_for_requirements(),
            crate::system_info::available_disk_gb(&partners_base_dir()),
        )
    }

    fn display_name(&self) -> &str {
        "SpaceAcres"
    }

    async fn install(&self) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            Self::quarantine_staged_installer();
            Self::quarantine_stale_staged_copy();
            let binary_found = match Self::installed_binary() {
                Some(existing) => {
                    info!(path = ?existing, "SpaceAcres already installed");
                    true
                }
                None => false,
            };
            if !install_needed(self.is_running(), binary_found) {
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

        if self.is_running() {
            info!("SpaceAcres already running");
            return Ok(());
        }

        info!(binary = ?binary, "Starting SpaceAcres");

        // Spawn the process with a base directory argument
        let base_dir = Self::partner_dir().join("data");
        std::fs::create_dir_all(&base_dir)?;

        // BUG 3: run with the farmer's own bin directory as CWD, not FEM's.
        // A WiX Burn-managed app can rely on its CWD to locate sibling
        // resources it expects next to itself — a wrong CWD is one of the
        // documented triggers for that kind of app's own self-verification
        // kicking off its Repair/Modify UI.
        let mut cmd = crate::supervisor::platform::command(&binary);
        cmd.arg("--base-directory").arg(&base_dir);
        if let Some(bin_dir) = binary.parent() {
            cmd.current_dir(bin_dir);
        }
        let child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to start SpaceAcres: {}", e))?;
        // B4 (D-03): SpaceAcres joins the kill-on-close job like every other
        // partner. The trade-off is deliberate and documented: an abrupt kill
        // can interrupt a plot. FEM's normal quit stops it gracefully first
        // (main.rs's ExitRequested handler), so the kernel kill only happens
        // when FEM itself died abnormally — which is precisely the case that
        // was leaving orphans behind before.
        crate::supervisor::platform::adopt_into_partner_job(&child);

        // BUG 3: track the child so is_running()/stop() can target it
        // directly instead of only an untargeted image-name scan.
        if let Ok(mut guard) = self.child.lock() {
            *guard = Some(child);
        }

        // Give it a moment to start
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        // BUG 3: prefer killing the exact tracked child when FEM spawned it
        // — targeted, no risk of taking down an unrelated process someone
        // else launched under the same image name. Falls back to the
        // image-name sweep for an adopted/untracked instance.
        let tracked = self.child.lock().ok().and_then(|mut g| g.take());
        if let Some(mut child) = tracked {
            let _ = child.kill();
            let _ = tokio::task::spawn_blocking(move || child.wait()).await;
            info!("Stopped SpaceAcres (tracked child)");
            return Ok(());
        }

        // Kill any running space-acres process (adoption fallback)
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

        info!("Stopped SpaceAcres (image-name sweep — no tracked child)");
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
        // BUG 7: only a DEFINITE "this machine has rotating disks" reading may
        // mark a running farmer unhealthy. `has_ssd()` collapsed unmeasurable
        // into false, so every NVMe box reported degraded forever.
        if ssd_state_for_requirements() == Some(false) {
            warn!("No SSD detected — SpaceAcres performance will be degraded");
            return HealthStatus::Unhealthy(
                "No SSD detected — SpaceAcres performance degraded".to_string(),
            );
        }
        if self.is_running() {
            HealthStatus::Healthy
        } else {
            HealthStatus::Stopped
        }
    }

    async fn check_update(&self) -> Result<Option<String>> {
        match Self::fetch_latest_release().await {
            Ok(release) => {
                // BUG 7: `installed_version()` used to return the literal
                // string "installed" (never a real version), so this always
                // looked like an update was available — "Installed → v0.2.21"
                // on every check, even on the latest release, and clicking
                // Update did nothing observable because there was nothing to
                // change. Compare against the REAL installed version now.
                let installed = self.installed_version();
                if update_available(installed.as_deref(), &release.version) {
                    info!(version = %release.version, installed = ?installed, "Found SpaceAcres update available");
                    Ok(Some(release.version))
                } else {
                    info!(installed = ?installed, latest = %release.version, "SpaceAcres already on the latest version");
                    Ok(None)
                }
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
        {
            let binary = Self::installed_binary()?;
            // BUG 7: read the REAL version off the binary actually in use,
            // instead of the literal placeholder string "installed" — that
            // placeholder compared unequal to every real release tag, so
            // `check_update()` (pre-fix) always reported an update
            // available even when already current.
            Self::file_product_version(&binary).or_else(|| Some("installed".into()))
        }
        #[cfg(not(target_os = "windows"))]
        {
            if Self::binary_path().exists() {
                Some("installed".into())
            } else {
                None
            }
        }
    }

    fn collect_poc_data(&self) -> PocGateData {
        PocGateData {
            poa: self.is_running(),
            ..Default::default()
        }
    }
}

/// Detect if system has an SSD.
/// Cached: the probe spawns a full PowerShell process (~1-3s) and this is
/// called from the 30s health-check loop — uncached it burns CPU forever,
/// and physical disks don't change while the app runs.
///
/// Primary: Get-PhysicalDisk | Where MediaType -eq 'SSD'
/// Fallback: MSFT_PhysicalDisk with SeekPenalty==false (indicates SSD)
/// SSD presence as a tri-state: `None` means the probe could not measure it.
///
/// `check_requirements()` must fail OPEN on an unmeasurable machine (the rule
/// `system_info` states: a transient probe error must never silently disable a
/// working integration), so "no SSD" and "could not tell" cannot share a value.
#[cfg(target_os = "windows")]
fn ssd_state() -> Option<bool> {
    if let Ok(guard) = SSD_CACHE.lock() {
        if let Some((value, at)) = *guard {
            if at.elapsed() < SSD_CACHE_TTL {
                return value;
            }
        }
    }
    let measured = (|| {
        // Primary probe answers definitively when it succeeds.
        let primary = crate::supervisor::platform::command("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-PhysicalDisk | Where MediaType -eq 'SSD' | Measure-Object | Select -Expand Count",
            ])
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<u32>()
                    .ok()
            });
        if let Some(count) = primary {
            if count > 0 {
                return Some(true);
            }
        }
        // Fallback: SeekPenalty == false also indicates an SSD.
        let fallback = crate::supervisor::platform::command("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-WmiObject -Namespace \"root/Microsoft/Windows/Storage\" -Class MSFT_PhysicalDisk | Where-Object SeekPenalty -EQ $false | Measure-Object).Count -gt 0",
            ])
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
            .ok()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .to_lowercase()
                    .contains("true")
            });
        // Third signal (BUG 7): a physical disk with SpindleSpeed 0 is an SSD
        // even when MediaType reads `Unspecified` — the NVMe / RAID / VMD case.
        let no_spindle = crate::supervisor::platform::command("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-PhysicalDisk | Where-Object { $_.SpindleSpeed -eq 0 } | Measure-Object).Count",
            ])
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u32>().ok())
            .unwrap_or(0);

        let spinning = crate::supervisor::platform::command("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-PhysicalDisk | Where-Object { $_.SpindleSpeed -gt 0 } | Measure-Object).Count",
            ])
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u32>().ok())
            .unwrap_or(0);

        classify_ssd(primary, fallback, no_spindle, spinning)
    })();
    if let Ok(mut guard) = SSD_CACHE.lock() {
        *guard = Some((measured, std::time::Instant::now()));
    }
    measured
}

/// PURE (BUG 7). Decide the tri-state SSD signal from the raw probe results.
///
/// `ssd_media`  = count of disks whose MediaType is literally 'SSD' (None = probe failed)
/// `no_penalty` = did the SeekPenalty==false probe find any device (None = probe failed)
/// `no_spindle` = count of physical disks reporting SpindleSpeed == 0
/// `spinning`   = count of physical disks reporting SpindleSpeed > 0
///
/// The critical rule the old code got wrong: a ZERO count is NOT evidence of a
/// spinning disk. On NVMe / RAID / VMD hardware every SSD-detecting probe can
/// legitimately return nothing, which is exactly what an HDD-only box looks
/// like. The two are only distinguishable by an AFFIRMATIVE rotational signal,
/// so `Some(false)` is returned solely when a disk actually reports a spindle
/// speed above zero. Everything else is `None` and fails OPEN.
fn classify_ssd(
    ssd_media: Option<u32>,
    no_penalty: Option<bool>,
    no_spindle: u32,
    spinning: u32,
) -> Option<bool> {
    if ssd_media.is_some_and(|c| c > 0) || no_spindle > 0 || no_penalty == Some(true) {
        return Some(true);
    }
    if spinning > 0 {
        return Some(false);
    }
    None
}

/// Warm the SSD probe before anything on a hot path can trigger it. The first
/// call spawns up to two PowerShell processes (~20s each at PROBE_TIMEOUT), and
/// `check_requirements()` runs inside `available_count()` under the registry
/// mutex — a cold probe there would freeze the UI and stall the PoC reporter.
pub fn warm_ssd_probe() {
    let _ = ssd_state_for_requirements();
}

/// Tri-state SSD signal for the requirements gate. Non-Windows reports `None`
/// (unmeasurable) rather than `false`: `has_ssd()` returns a bare `false`
/// there because detection was never implemented, and treating that as "no
/// SSD" would mark SpaceAcres permanently unavailable for every Linux and
/// macOS user.
fn ssd_state_for_requirements() -> Option<bool> {
    #[cfg(target_os = "windows")]
    {
        ssd_state()
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// BUG 7: a TTL cache, not a process-lifetime `OnceLock`.
///
/// The probe is warmed at the coldest possible moment (`main.rs` setup). A
/// cold `Get-PhysicalDisk` that exceeds PROBE_TIMEOUT, or a Storage WMI
/// namespace that is briefly unavailable, used to poison the answer until the
/// app was restarted. Re-probing every 10 minutes matches `system_info`'s own
/// disk/RAM caches and costs at most one extra PowerShell spawn per window.
#[cfg(target_os = "windows")]
const SSD_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(600);

#[cfg(target_os = "windows")]
static SSD_CACHE: std::sync::Mutex<Option<(Option<bool>, std::time::Instant)>> =
    std::sync::Mutex::new(None);

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
            candidates.contains(&PathBuf::from(
                r"C:\Program Files\Space Acres\bin\space-acres.exe"
            )),
            "candidates must include the WiX bin layout: {candidates:?}"
        );
        assert!(
            candidates.contains(&PathBuf::from(
                r"C:\Program Files\Space Acres\space-acres.exe"
            )),
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
            roots.contains(&PathBuf::from(
                r"C:\Users\x\AppData\Local\Programs\Space Acres"
            )),
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
        let head =
            b"MZ\x90\x00.text\x00\x00\x00.rdata\x00\x00.data\x00\x00\x00.rsrc\x00\x00\x00".to_vec();
        assert!(!head_is_burn_bundle(&head));
    }

    #[test]
    fn staged_installer_is_skipped_in_favour_of_the_real_install() {
        let staged = PathBuf::from(
            r"C:\Users\u\AppData\Roaming\FryEdgeMiner\partners\space_acres\space-acres.exe",
        );
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

    /// Bug 4: `head_is_valid_pe` is the trust gate a corrupted staged file
    /// must fail before it can ever reach `pick_binary`.
    #[test]
    fn head_is_valid_pe_requires_the_mz_magic() {
        assert!(head_is_valid_pe(
            b"MZ\x90\x00.text\x00\x00\x00.rdata\x00\x00"
        ));
        // WP6/WP3-style corruption: plain ASCII, no MZ header.
        assert!(!head_is_valid_pe(
            b"WP6-TEST-CORRUPTION-NOT-A-VALID-EXECUTABLE"
        ));
        assert!(!head_is_valid_pe(b""));
        // Too short to even carry the 2-byte magic.
        assert!(!head_is_valid_pe(b"M"));
    }

    /// Bug 4 regression: a staged file that is corrupted (truncated/garbage,
    /// no MZ header — WP3's exact repro) must NOT be preferred over a
    /// known-good discovered install, mirroring the existing
    /// `staged_installer_is_skipped_in_favour_of_the_real_install` test for
    /// the Burn-bundle case. `pick_binary`'s own signature is unchanged —
    /// only what `installed_binary()` passes as the second argument changes
    /// (staged_is_installer || staged_is_corrupt), so this exercises the
    /// exact combined flag the production call site now computes.
    #[test]
    fn a_non_pe_staged_file_is_not_trusted_over_a_real_install() {
        let staged = PathBuf::from(
            r"C:\Users\u\AppData\Roaming\FryEdgeMiner\partners\space_acres\space-acres.exe",
        );
        let discovered = PathBuf::from(r"C:\Program Files\Space Acres\bin\space-acres.exe");
        let staged_is_installer = false; // not a Burn bundle — the OLD check would have trusted it
        let staged_is_corrupt = !head_is_valid_pe(b"WP6-TEST-CORRUPTION-NOT-A-VALID-EXECUTABLE");
        let staged_untrusted = staged_is_installer || staged_is_corrupt;
        assert!(staged_untrusted, "a non-PE staged file must be untrusted");
        assert_eq!(
            pick_binary(Some(staged), staged_untrusted, Some(discovered.clone())),
            Some(discovered)
        );
    }

    /// Negative control: a staged file WITH a valid MZ header (a real farmer
    /// binary) must still win over the discovered copy, exactly as
    /// `staged_farmer_still_wins_when_it_is_a_real_binary` already proves for
    /// the Burn-bundle flag — confirms this fix does not make the trust gate
    /// stricter than intended.
    #[test]
    fn a_valid_pe_staged_farmer_is_still_preferred() {
        let staged = PathBuf::from(r"C:\staged\space-acres.exe");
        let discovered = PathBuf::from(r"C:\Program Files\Space Acres\bin\space-acres.exe");
        let staged_is_installer = false;
        let staged_is_corrupt = !head_is_valid_pe(b"MZ\x90\x00real-farmer-bytes");
        let staged_untrusted = staged_is_installer || staged_is_corrupt;
        assert!(!staged_untrusted, "a valid PE staged file must be trusted");
        assert_eq!(
            pick_binary(Some(staged.clone()), staged_untrusted, Some(discovered)),
            Some(staged)
        );
    }
}

#[cfg(all(test, target_os = "windows"))]
mod pe_completeness_tests {
    //! WP3 (2026-09-07) live finding: the shipped Bug-4 gate is a 2-byte "MZ"
    //! check, so a staged farmer that was TRUNCATED mid-write (the exact
    //! corruption reproduced on 2026-08-31) keeps its MZ magic, passes the
    //! gate, and is still preferred over the Program Files install — FEM then
    //! spawns a 200-byte file. These tests exercise `file_is_valid_pe`, the
    //! file-level predicate `installed_binary()` actually calls.
    use super::*;
    use std::io::Write;

    /// Minimal synthetic PE: 64-byte DOS header with e_lfanew = 0x40, "PE\0\0",
    /// a COFF header declaring ONE section and no optional header, and one
    /// section header whose raw data spans [0x200, 0x1200). A complete file is
    /// therefore exactly 0x1200 = 4608 bytes.
    fn synthetic_pe(total_len: usize) -> Vec<u8> {
        let mut v = vec![0u8; 0x40];
        v[0] = b'M';
        v[1] = b'Z';
        v[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        v.extend_from_slice(b"PE\0\0");
        let mut coff = [0u8; 20];
        coff[2..4].copy_from_slice(&1u16.to_le_bytes()); // NumberOfSections
        coff[16..18].copy_from_slice(&0u16.to_le_bytes()); // SizeOfOptionalHeader
        v.extend_from_slice(&coff);
        let mut sect = [0u8; 40];
        sect[..5].copy_from_slice(b".text");
        sect[16..20].copy_from_slice(&0x1000u32.to_le_bytes()); // SizeOfRawData
        sect[20..24].copy_from_slice(&0x200u32.to_le_bytes()); // PointerToRawData
        v.extend_from_slice(&sect);
        v.resize(0x1200, 0xCC);
        v.truncate(total_len);
        v
    }

    fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fem-pe-completeness-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let p = dir.join(name);
        std::fs::File::create(&p)
            .and_then(|mut f| f.write_all(bytes))
            .expect("write temp pe");
        p
    }

    #[test]
    fn a_complete_pe_file_is_a_valid_staged_binary() {
        let p = write_temp("space-acres.exe", &synthetic_pe(0x1200));
        assert!(file_is_valid_pe(&p), "a complete PE must stay trusted");
    }

    #[test]
    fn a_truncated_pe_file_is_not_a_valid_staged_binary() {
        // WP3 trial (a): the first 200 bytes of a real farmer binary — MZ magic
        // intact, everything after the headers gone.
        let p = write_temp("space-acres.exe", &synthetic_pe(200));
        assert!(
            !file_is_valid_pe(&p),
            "a PE whose section table declares bytes beyond EOF was TRUNCATED and must not be trusted"
        );
    }

    #[test]
    fn a_pe_truncated_inside_its_section_data_is_not_a_valid_staged_binary() {
        // Headers fully present, raw data cut short (disk-full / power-loss mid-write).
        let p = write_temp("space-acres.exe", &synthetic_pe(0x900));
        assert!(!file_is_valid_pe(&p));
    }

    #[test]
    fn a_pe_whose_header_offsets_point_past_eof_is_not_a_valid_staged_binary() {
        let mut bytes = synthetic_pe(0x1200);
        bytes[0x3C..0x40].copy_from_slice(&0x7FFF_0000u32.to_le_bytes()); // e_lfanew far beyond the file
        let p = write_temp("space-acres.exe", &bytes);
        assert!(!file_is_valid_pe(&p));
    }
}

#[cfg(test)]
mod requirement_tests {
    use super::*;

    /// E11: SpaceAcres inherited the trait's default `Ok(())`, so a machine
    /// that can never run it still counted in `available_count()` — the
    /// denominator of the multiplier the device submits. Under-provisioned
    /// devices were therefore paid LESS than they earned.
    #[test]
    fn a_machine_without_an_ssd_is_unavailable() {
        let err = evaluate_requirements(Some(false), Some(500.0)).unwrap_err();
        assert!(err.contains("SSD"), "unexpected reason: {err}");
    }

    #[test]
    fn a_machine_below_the_disk_minimum_is_unavailable() {
        let err = evaluate_requirements(Some(true), Some(10.0)).unwrap_err();
        assert!(
            err.contains("10"),
            "reason should quote the free space: {err}"
        );
        assert!(err.contains(&SPACE_ACRES_MIN_GB.to_string()));
    }

    #[test]
    fn a_correctly_provisioned_machine_passes() {
        assert!(evaluate_requirements(Some(true), Some(500.0)).is_ok());
    }

    #[test]
    fn exactly_at_the_threshold_is_accepted() {
        assert!(evaluate_requirements(Some(true), Some(SPACE_ACRES_MIN_GB as f64)).is_ok());
    }

    /// Fail-open: an unmeasurable probe must never silently disable a working
    /// integration (the rule `system_info` documents, and what Iagon does).
    /// Non-Windows reports `None` for the SSD signal, so this also keeps
    /// SpaceAcres available on Linux/macOS instead of permanently unavailable.
    #[test]
    fn unmeasurable_specs_fail_open() {
        assert!(evaluate_requirements(None, None).is_ok());
        assert!(
            evaluate_requirements(None, Some(10.0)).is_err(),
            "a measured shortfall still fails"
        );
        assert!(evaluate_requirements(Some(true), None).is_ok());
    }
}

/// BUG 3: SpaceAcres repair-loop tests — tracked-child liveness, fail-closed
/// image-name probe, and staged-vs-discovered version quarantine.
#[cfg(test)]
mod bug3_tracked_child_tests {
    use super::*;

    /// A real, short-lived child process — exercises `tracked_child_probe`'s
    /// actual `try_wait()` semantics rather than a mock.
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

    #[test]
    fn no_tracked_child_means_fall_back_to_the_image_name_probe() {
        let mut slot: Option<std::process::Child> = None;
        assert_eq!(tracked_child_probe(&mut slot), None);
    }

    #[test]
    fn a_still_running_tracked_child_reports_running_without_a_probe_fallback() {
        let mut slot = Some(spawn_long_lived());
        assert_eq!(tracked_child_probe(&mut slot), Some(true));
        // Clean up so the test doesn't leak a 30s sleep process.
        if let Some(mut c) = slot.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    #[test]
    fn an_exited_tracked_child_reports_not_running_and_clears_the_slot() {
        let mut child = spawn_short_lived();
        // Give it a moment to actually exit before probing.
        let _ = child.wait();
        let mut slot = Some(child);
        assert_eq!(tracked_child_probe(&mut slot), Some(false));
        assert!(
            slot.is_none(),
            "an exited child must be cleared from the slot"
        );
    }

    // --- staged_is_stale (pure) ----------------------------------------------

    #[test]
    fn matching_versions_are_not_stale() {
        assert!(!staged_is_stale(Some("0.2.21"), Some("0.2.21")));
    }

    #[test]
    fn differing_versions_are_stale() {
        assert!(staged_is_stale(Some("0.2.20"), Some("0.2.21")));
    }

    #[test]
    fn an_unmeasurable_version_on_either_side_never_triggers_quarantine() {
        assert!(!staged_is_stale(None, Some("0.2.21")));
        assert!(!staged_is_stale(Some("0.2.21"), None));
        assert!(!staged_is_stale(None, None));
    }
}

/// BUG 7 (Discord: "Installed → v0.2.21" shown on every check, Update
/// button does nothing): `check_update`/`installed_version` version-aware
/// comparison.
#[cfg(test)]
mod bug7_update_check_tests {
    use super::*;

    #[test]
    fn the_same_version_on_both_sides_offers_no_update() {
        assert!(!update_available(Some("0.2.21"), "v0.2.21"));
    }

    #[test]
    fn a_trailing_zero_component_from_the_pe_resource_does_not_cause_a_false_update() {
        // Windows ProductVersion resources commonly carry a 4th ".0"
        // component a git tag never has — this is EXACTLY the shape that
        // made the pre-fix "installed" placeholder string always compare
        // unequal to a real tag.
        assert!(!update_available(Some("0.2.21.0"), "v0.2.21"));
    }

    #[test]
    fn a_genuinely_newer_release_is_offered() {
        assert!(update_available(Some("0.2.20"), "v0.2.21"));
    }

    #[test]
    fn a_genuinely_older_installed_version_string_is_not_offered_a_downgrade() {
        assert!(!update_available(Some("0.3.0"), "v0.2.21"));
    }

    #[test]
    fn an_unmeasurable_installed_version_fails_open_toward_offering_the_update() {
        // Matches the pre-fix behavior for this specific case — an offer is
        // reversible (Update does nothing harmful if already current),
        // silently withholding one on an unmeasurable machine is worse.
        assert!(update_available(None, "v0.2.21"));
    }

    #[test]
    fn version_component_parsing_strips_a_leading_v_and_pads_nothing_itself() {
        assert_eq!(parse_version_components("v0.2.21"), vec![0, 2, 21]);
        assert_eq!(parse_version_components("0.2.21.0"), vec![0, 2, 21, 0]);
    }
}

/// BUG 7 (minerman): "No SSD detected" on a device that HAS an SSD.
///
/// The old classifier collapsed "could not tell" into "definitively no SSD":
/// the arm `(Some(_), Some(false))` committed to `Some(false)`. On NVMe drives
/// and disks behind RAID/Intel-RST/VMD controllers `Get-PhysicalDisk` reports
/// `MediaType = Unspecified` (so the count is 0) and `MSFT_PhysicalDisk`
/// reports `SeekPenalty = $null` (so `-EQ $false` matches nothing) — two
/// non-answers that together produced a confident "no".
///
/// Measured on FryStation, which has two real SSDs (one NVMe, one SATA):
///   Get-PhysicalDisk | Where MediaType -eq 'SSD'  -> 2
///   Win32_DiskDrive.MediaType                     -> "Fixed hard disk media" (both)
///   Win32_DiskDrive.InterfaceType                 -> IDE (for the SATA SSD)
/// i.e. the legacy WMI view is useless and only the tri-state reading is safe.
#[cfg(test)]
mod bug7_ssd_classifier_tests {
    use super::*;

    #[test]
    fn a_positive_count_is_a_definite_yes() {
        assert_eq!(classify_ssd(Some(2), None, 0, 0), Some(true));
        assert_eq!(classify_ssd(Some(1), Some(false), 0, 0), Some(true));
    }

    /// The reported bug. An NVMe box: the SSD count is 0 because MediaType is
    /// `Unspecified`, and SeekPenalty is null so the fallback matches nothing.
    /// Neither probe actually SAW a spinning disk, so the honest answer is
    /// "unmeasurable" — which fails OPEN — not "no SSD", which fails closed.
    #[test]
    fn an_nvme_box_reporting_unspecified_media_is_unmeasurable_not_a_definite_no() {
        assert_eq!(
            classify_ssd(Some(0), Some(false), 0, 0),
            None,
            "BUG 7: two non-answers must not add up to a confident 'no SSD'"
        );
    }

    /// A third signal: physical disks reporting a zero spindle speed are SSDs
    /// even when MediaType is Unspecified.
    #[test]
    fn a_zero_spindle_speed_rescues_a_box_the_first_two_probes_missed() {
        assert_eq!(classify_ssd(Some(0), Some(false), 1, 0), Some(true));
        assert_eq!(classify_ssd(None, None, 2, 0), Some(true));
    }

    /// A genuine spinning-rust machine must still be a definite no, or the
    /// requirements gate stops protecting anyone.
    #[test]
    fn a_real_spinning_disk_machine_is_still_a_definite_no() {
        // Only an AFFIRMATIVE spindle-speed reading justifies a confident no.
        assert_eq!(classify_ssd(Some(0), Some(false), 0, 2), Some(false));
    }

    /// The distinction the old code could not make: an NVMe box and an
    /// HDD-only box produce identical SSD-probe output, and are told apart
    /// only by whether any disk affirmatively reports a spindle speed.
    #[test]
    fn an_nvme_box_and_a_spinning_box_are_told_apart_by_the_rotational_signal() {
        let nvme = classify_ssd(Some(0), Some(false), 0, 0);
        let hdd = classify_ssd(Some(0), Some(false), 0, 3);
        assert_eq!(nvme, None, "NVMe: unmeasurable, must fail open");
        assert_eq!(
            hdd,
            Some(false),
            "HDD: affirmative rotation, must fail closed"
        );
        assert_ne!(nvme, hdd);
    }

    #[test]
    fn nothing_measurable_stays_unmeasurable() {
        assert_eq!(classify_ssd(None, None, 0, 0), None);
    }

    /// BUG 1/4 + BUG 7: the toggle gate and the requirements gate must agree.
    #[test]
    fn eligibility_and_requirements_never_disagree() {
        let cases = [
            (Some(true), Some(100.0)),
            (Some(true), Some(49.0)),
            (Some(false), Some(500.0)),
            (None, None),
        ];
        for (ssd, gb) in cases {
            let verdict = evaluate_requirements(ssd, gb);
            let expected = match &verdict {
                Ok(()) => (true, None),
                Err(r) => (false, Some(r.clone())),
            };
            assert_eq!(eligibility_from(verdict), expected, "ssd={ssd:?} gb={gb:?}");
        }
    }

    /// An unmeasurable machine must NOT be blocked by the toggle when the real
    /// requirements gate would let it through. This is the off-Windows and
    /// NVMe case that used to be permanently untoggleable.
    #[test]
    fn an_unmeasurable_machine_is_no_longer_blocked_by_the_toggle() {
        assert_eq!(
            eligibility_from(evaluate_requirements(None, None)),
            (true, None)
        );
    }
}
