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

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(600);

static DISK_CACHE: Mutex<Option<(f64, Instant)>> = Mutex::new(None);
static RAM_CACHE: Mutex<Option<(f64, Instant)>> = Mutex::new(None);

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
    if let Some(hit) = cached(&DISK_CACHE) {
        return Some(hit);
    }
    let measured = probe_disk_gb(path)?;
    store(&DISK_CACHE, measured);
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
    let path = path.to_string_lossy().to_string();
    let drive = path.split(':').next().unwrap_or("C");
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
        let df = "Filesystem 1G-blocks Used Available Use% Mounted on\n/dev/sda1 500G 372G 128G 75% /\n";
        assert_eq!(parse_df_gb(df), Some(128.0));
    }

    #[test]
    fn df_without_a_data_row_is_none() {
        assert_eq!(parse_df_gb("Filesystem 1G-blocks Used Available Use% Mounted on\n"), None);
    }

    #[test]
    fn parses_meminfo_total() {
        let mem = "MemTotal:       16311248 kB\nMemFree:         1234 kB\n";
        let gb = parse_meminfo_gb(mem).unwrap();
        assert!((gb - 15.55).abs() < 0.05, "got {gb}");
    }
}
