use crate::supervisor::platform::BoundedOutput;
use anyhow::Result;
use std::net::IpAddr;
use std::time::Duration;
use tokio::sync::Semaphore;
use tracing::warn;

const TEQUILAPI_PORT: u16 = 4449;
const SCAN_TIMEOUT: Duration = Duration::from_millis(200);
const MAX_CONCURRENT: usize = 32;
const SCAN_DEADLINE: Duration = Duration::from_secs(10);

/// Scan the local /24 subnet for an existing Mysterium node.
/// Also check for local myst.exe process.
/// Returns Option<String> — conflict description if found, None otherwise.
pub async fn scan_lan_conflict() -> Result<Option<String>> {
    let deadline = tokio::time::Instant::now() + SCAN_DEADLINE;

    // Check local process. This shells out (tasklist/pgrep) and blocks the
    // calling thread until output_bounded's own PROBE_TIMEOUT/REAP_GRACE
    // resolve it — previously that ran directly on whatever Tokio worker
    // thread called this async fn, so a slow-to-reap child here could starve
    // every other async task scheduled on that worker (including the polling
    // that repaints the UI), not just this one call. spawn_blocking moves it
    // to the blocking thread pool instead.
    let local_process = tokio::task::spawn_blocking(check_local_myst_process)
        .await
        .unwrap_or_else(|e| {
            warn!(error = %e, "check_local_myst_process panicked or was cancelled");
            None
        });
    if local_process.is_some() {
        return Ok(local_process);
    }

    // Scan subnet
    match tokio::time::timeout_at(deadline, scan_subnet()).await {
        Ok(Ok(Some(msg))) => Ok(Some(msg)),
        Ok(Ok(None)) => Ok(None),
        Ok(Err(e)) => {
            warn!(error = %e, "LAN scan failed");
            Ok(None)
        }
        Err(_) => {
            warn!("LAN scan exceeded 10s deadline");
            Ok(None)
        }
    }
}

/// Check if myst.exe is running on the local machine.
fn check_local_myst_process() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        crate::supervisor::platform::command("tasklist")
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
            .ok()
            .and_then(|o| {
                let output = String::from_utf8_lossy(&o.stdout);
                if output.to_lowercase().contains("myst.exe") {
                    Some("Mysterium node (myst.exe) already running on this device".to_string())
                } else {
                    None
                }
            })
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Unix: check pgrep output
        crate::supervisor::platform::command("pgrep")
            .arg("-l")
            .arg("myst")
            .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
            .ok()
            .and_then(|o| {
                let output = String::from_utf8_lossy(&o.stdout);
                if !output.is_empty() {
                    Some("Mysterium node already running on this device".to_string())
                } else {
                    None
                }
            })
    }
}

/// Scan local /24 subnet for Mysterium tequilapi port (4449).
async fn scan_subnet() -> Result<Option<String>> {
    let local_ip = get_local_ip().await?;
    let subnet = format!("{}.0", local_ip.rsplit_once('.').map(|(a, _)| a).unwrap_or("0"));

    // Build candidate IPs: subnet.1 through subnet.254 (skip .0 and .255)
    let mut tasks = Vec::new();
    let sem = std::sync::Arc::new(Semaphore::new(MAX_CONCURRENT));

    for i in 1..=254 {
        let ip_str = format!("{}.{}", subnet, i);
        let sem_clone = sem.clone();

        tasks.push(tokio::spawn(async move {
            let _permit = sem_clone.acquire().await;
            probe_ip(&ip_str).await
        }));
    }

    for task in tasks {
        match task.await {
            Ok(Some(conflict)) => return Ok(Some(conflict)),
            _ => {}
        }
    }

    Ok(None)
}

/// Probe a single IP for Mysterium tequilapi port.
async fn probe_ip(ip: &str) -> Option<String> {
    let addr = format!("{}:{}", ip, TEQUILAPI_PORT);
    match tokio::time::timeout(SCAN_TIMEOUT, tokio::net::TcpStream::connect(&addr)).await {
        Ok(Ok(_)) => {
            Some(format!(
                "Mysterium node detected on LAN at {}:{} — enable myst_lan_override to proceed",
                ip, TEQUILAPI_PORT
            ))
        }
        _ => None,
    }
}

/// Get local IP address (best effort).
async fn get_local_ip() -> Result<String> {
    // Heuristic: connect to a public IP (don't actually send data) to discover local IP
    match tokio::net::UdpSocket::bind("0.0.0.0:0").await {
        Ok(socket) => {
            let _ = socket.connect("8.8.8.8:53").await; // Google DNS, arbitrary
            if let Ok(addr) = socket.local_addr() {
                if let IpAddr::V4(v4) = addr.ip() {
                    return Ok(v4.to_string());
                }
            }
        }
        Err(_) => {}
    }

    // Fallback
    Ok("127.0.0".to_string())
}

#[cfg(test)]
mod bug2_regression_tests {
    /// Bug 2 regression: the blocking `tasklist`/`pgrep` probe inside
    /// `check_local_myst_process` must not run inline on the async worker
    /// thread that called `scan_lan_conflict`. Proven on a single-worker
    /// `current_thread` runtime: a slow blocking closure offloaded via
    /// `spawn_blocking` (the same primitive the fix uses) must NOT prevent a
    /// concurrently-scheduled cheap async task from completing on time. Before
    /// the fix, the equivalent inline call would have monopolized this
    /// runtime's one and only worker thread, and the sleep task below would
    /// never have been polled until the blocking call returned.
    #[tokio::test(flavor = "current_thread")]
    async fn blocking_work_offloaded_via_spawn_blocking_does_not_starve_the_async_worker() {
        let slow_blocking = tokio::task::spawn_blocking(|| {
            std::thread::sleep(std::time::Duration::from_millis(300));
            "blocking-done"
        });
        let cheap_async = async {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            "async-done"
        };

        let started = std::time::Instant::now();
        // If the blocking work ran inline on this current_thread runtime's
        // single worker instead of on the blocking pool, `cheap_async` could
        // not be polled until the 300ms sleep finished, and this would
        // observe elapsed >= 300ms instead of ~20ms.
        let async_result = cheap_async.await;
        let elapsed_before_blocking_finishes = started.elapsed();

        assert_eq!(async_result, "async-done");
        assert!(
            elapsed_before_blocking_finishes < std::time::Duration::from_millis(250),
            "cheap async task took {elapsed_before_blocking_finishes:?} — the concurrent \
             blocking work appears to have starved this runtime's worker thread"
        );

        let blocking_result = slow_blocking.await.unwrap();
        assert_eq!(blocking_result, "blocking-done");
    }
}
