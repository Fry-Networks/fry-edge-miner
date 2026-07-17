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

    // Check local process
    let local_process = check_local_myst_process();
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
            .output()
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
            .output()
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
