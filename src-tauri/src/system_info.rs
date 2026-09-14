//! Host capability probes used by `Integration::check_requirements`.
//!
//! Both probes shell out (PowerShell on Windows, `df`/`/proc` elsewhere), and
//! `check_requirements` is called from the PoC reporter on every report as well
//! as from the UI status command — so the results are memoised. Disk headroom
//! and installed RAM do not move fast enough for a 10-minute window to matter.
//!
//! Every probe returns `Option`: a probe that fails yields `None`, and callers
//! treat `None` as "cannot prove the machine is unfit" and allow the
//! integration through. Failing open matters — a transient PowerShell error
//! must never silently disable a working integration.
use crate::supervisor::platform::BoundedOutput;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(600);

/// BUG 1/4: keyed by volume, not a single global slot. `HashMap::new` is not
/// const, so this uses the `OnceLock` idiom already used by `space_acres`.
static DISK_CACHE: std::sync::OnceLock<Mutex<HashMap<String, (f64, Instant)>>> =
    std::sync::OnceLock::new();
static RAM_CACHE: Mutex<Option<(f64, Instant)>> = Mutex::new(None);

fn disk_cache() -> &'static Mutex<HashMap<String, (f64, Instant)>> {
    DISK_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The volume a path lives on, e.g. `D:\FryEdgeMiner\partners` -> `Some("D")`.
///
/// PURE, and the single source of truth: BOTH the cache key and the Windows
/// probe call this. Before, the probe had its own inline `split(':')` and the
/// cache had no key at all, so the two could not be kept in agreement.
pub(crate) fn drive_letter(path: &Path) -> Option<String> {
    let s = path.to_string_lossy();
    let (head, _) = s.split_once(':')?;
    let head = head.trim_start_matches(['\\', '/']);
    let mut chars = head.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => Some(c.to_ascii_uppercase().to_string()),
        _ => None,
    }
}

/// Cache key: the volume, or — when there is none (UNC) — the path itself,
/// which can never collide with a single-letter key.
fn volume_key(path: &Path) -> String {
    match drive_letter(path) {
        Some(letter) => letter,
        None => path.to_string_lossy().to_ascii_lowercase(),
    }
}

/// PURE lookup, split out so keying and the TTL are testable without a disk.
fn cache_lookup(map: &HashMap<String, (f64, Instant)>, key: &str, now: Instant) -> Option<f64> {
    let &(value, at) = map.get(key)?;
    (now.duration_since(at) < CACHE_TTL).then_some(value)
}

fn cached(slot: &Mutex<Option<(f64, Instant)>>) -> Option<f64> {
    let guard = slot.lock().ok()?;
    let (value, at) = (*guard)?;
    (at.elapsed() < CACHE_TTL).then_some(value)
}

fn store(slot: &Mutex<Option<(f64, Instant)>>, value: f64) {
    if let Ok(mut guard) = slot.lock() {
        *guard = Some((value, Instant::now()));
    }
}

/// Free space in GB on the volume holding `path`.
pub fn available_disk_gb(path: &Path) -> Option<f64> {
    let key = volume_key(path);
    if let Ok(guard) = disk_cache().lock() {
        if let Some(hit) = cache_lookup(&guard, &key, Instant::now()) {
            return Some(hit);
        }
    }
    let measured = probe_disk_gb(path)?;
    if let Ok(mut guard) = disk_cache().lock() {
        guard.insert(key, (measured, Instant::now()));
    }
    Some(measured)
}

/// Total installed physical memory in GB.
pub fn total_ram_gb() -> Option<f64> {
    if let Some(hit) = cached(&RAM_CACHE) {
        return Some(hit);
    }
    let measured = probe_ram_gb()?;
    store(&RAM_CACHE, measured);
    Some(measured)
}

#[cfg(target_os = "windows")]
fn probe_disk_gb(path: &Path) -> Option<f64> {
    let drive = drive_letter(path).unwrap_or_else(|| "C".to_string());
    let output = crate::supervisor::platform::command("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "(Get-Volume -DriveLetter {} | Select-Object -Expand SizeRemaining) / 1GB",
                drive
            ),
        ])
        .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
        .ok()?;
    parse_float(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(target_os = "windows"))]
fn probe_disk_gb(path: &Path) -> Option<f64> {
    let output = crate::supervisor::platform::command("df")
        .arg("-BG")
        .arg(path)
        .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
        .ok()?;
    parse_df_gb(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(target_os = "windows")]
fn probe_ram_gb() -> Option<f64> {
    let output = crate::supervisor::platform::command("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB",
        ])
        .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
        .ok()?;
    parse_float(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(target_os = "windows"))]
fn probe_ram_gb() -> Option<f64> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    parse_meminfo_gb(&meminfo)
}

/// PowerShell emits the invariant/locale-formatted number on its own line.
fn parse_float(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Some locales render the decimal separator as a comma.
    trimmed
        .parse::<f64>()
        .or_else(|_| trimmed.replacen(',', ".", 1).parse::<f64>())
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
}

/// `df -BG` second line, 4th column ("Available"), e.g. `128G`.
#[cfg_attr(target_os = "windows", allow(dead_code))]
fn parse_df_gb(raw: &str) -> Option<f64> {
    let line = raw.lines().nth(1)?;
    let avail = line.split_whitespace().nth(3)?;
    avail.trim_end_matches(['G', 'B']).parse::<f64>().ok()
}

/// `MemTotal:  16311248 kB` -> GB.
#[cfg_attr(target_os = "windows", allow(dead_code))]
fn parse_meminfo_gb(raw: &str) -> Option<f64> {
    let line = raw.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kb = line.split_whitespace().nth(1)?.parse::<f64>().ok()?;
    Some(kb / 1024.0 / 1024.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_powershell_float() {
        assert_eq!(parse_float("127.94140625\r\n"), Some(127.94140625));
        assert_eq!(parse_float(" 900 "), Some(900.0));
    }

    #[test]
    fn parses_comma_decimal_locales() {
        assert_eq!(parse_float("127,5\r\n"), Some(127.5));
    }

    #[test]
    fn rejects_empty_or_garbage_output() {
        assert_eq!(parse_float(""), None);
        assert_eq!(parse_float("   \r\n"), None);
        assert_eq!(parse_float("Get-Volume : not recognized"), None);
    }

    #[test]
    fn parses_df_available_column() {
        let df =
            "Filesystem 1G-blocks Used Available Use% Mounted on\n/dev/sda1 500G 372G 128G 75% /\n";
        assert_eq!(parse_df_gb(df), Some(128.0));
    }

    #[test]
    fn df_without_a_data_row_is_none() {
        assert_eq!(
            parse_df_gb("Filesystem 1G-blocks Used Available Use% Mounted on\n"),
            None
        );
    }

    #[test]
    fn parses_meminfo_total() {
        let mem = "MemTotal:       16311248 kB\nMemFree:         1234 kB\n";
        let gb = parse_meminfo_gb(mem).unwrap();
        assert!((gb - 15.55).abs() < 0.05, "got {gb}");
    }
}

/// BUG 1/4: `DISK_CACHE` used to be a single global slot with NO key, and
/// `available_disk_gb` consulted it BEFORE looking at `path`. Once a second
/// storage location existed, whichever drive probed first served its answer to
/// every other drive for the full 600 s TTL — so a user who pointed FEM at D:
/// would still be told C:'s free space, and Iagon's 900 GB gate would keep
/// reading the wrong volume.
#[cfg(test)]
mod bug1_disk_cache_tests {
    use super::*;
    use std::collections::HashMap;

    /// The anti-divergence invariant. The cache key and the Windows probe must
    /// derive the volume through the SAME function — previously the probe had
    /// its own inline `split(':')` and the cache had no key at all, so they
    /// could not be kept in agreement even in principle.
    #[test]
    fn the_cache_key_and_the_probe_read_the_same_drive() {
        for p in [
            r"C:\Users\u\AppData\Roaming",
            r"D:\FryEdgeMiner\partners",
            r"e:\x",
        ] {
            let path = Path::new(p);
            let letter = drive_letter(path).expect("a lettered path must yield a letter");
            assert_eq!(
                volume_key(path),
                letter,
                "the cache key must be exactly the drive the probe will query"
            );
        }
    }

    /// The headline regression test for the landmine.
    #[test]
    fn two_drives_never_serve_each_others_answer() {
        // FryStation's real numbers, so the fixture is not hypothetical.
        let now = Instant::now();
        let mut m: HashMap<String, (f64, Instant)> = HashMap::new();
        m.insert("C".to_string(), (71.0, now));
        m.insert("D".to_string(), (1863.0, now));

        assert_eq!(
            cache_lookup(&m, "D", now),
            Some(1863.0),
            "D: must get D:'s answer"
        );
        assert_eq!(
            cache_lookup(&m, "C", now),
            Some(71.0),
            "C: must get C:'s answer"
        );
    }

    /// TTL preservation: re-keying must not turn one probe per 600 s into one
    /// PowerShell spawn per integration. `check_requirements()` runs sync under
    /// the registry mutex on every UI poll and every PoC report.
    #[test]
    fn paths_on_the_same_drive_share_one_probe() {
        assert_eq!(
            volume_key(Path::new(r"D:\a\b")),
            volume_key(Path::new(r"D:\c"))
        );
        assert_eq!(
            volume_key(Path::new(r"d:\a")),
            volume_key(Path::new(r"D:\b"))
        );
    }

    #[test]
    fn an_expired_entry_is_not_served() {
        let now = Instant::now();
        let mut m: HashMap<String, (f64, Instant)> = HashMap::new();
        m.insert("D".to_string(), (1863.0, now));
        let later = now + CACHE_TTL + Duration::from_secs(1);
        assert_eq!(cache_lookup(&m, "D", later), None);
        assert_eq!(
            cache_lookup(&m, "D", now + Duration::from_secs(1)),
            Some(1863.0)
        );
    }

    /// A UNC path has no drive letter. It must key on itself so it can never
    /// collide with a single-letter volume key.
    #[test]
    fn a_unc_path_keys_on_itself_and_never_collides_with_a_lettered_volume() {
        let unc = Path::new(r"\nas\share\fem");
        assert_eq!(drive_letter(unc), None);
        let key = volume_key(unc);
        assert!(key.len() > 1, "a UNC key must not look like a drive letter");
        assert_ne!(key, "C");
    }
}
