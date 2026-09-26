//! B10: who is holding a TCP port, and how to say so on the card.
//!
//! frynode binds `:8088`. Nothing in FEM ever asked what happens when that
//! port is already taken — by a leftover frynode from a hard-killed FEM, or by
//! an unrelated program — so the card could only ever say "frynode process is
//! not running", the supervisor restarted it every 30 s into the same failure,
//! and the user was left toggling until something happened to free the port.

#[cfg(target_os = "windows")]
use crate::supervisor::platform::{command, BoundedOutput};

/// Deliberately far below `platform::PROBE_TIMEOUT` (20 s): this runs on the
/// boot auto-start path and inside the 60 s toggle budget, so it must never be
/// the reason either of those runs out.
#[cfg(target_os = "windows")]
const OWNER_LOOKUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Who holds a port, as far as we could tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PortState {
    /// Bindable right now.
    Free,
    /// Taken, and we resolved the owner.
    HeldBy { pid: u32, image: String },
    /// Taken, but the owner could not be resolved — a socket owned by another
    /// user or a higher integrity level is not always visible to FEM's
    /// non-elevated session, and that must degrade, not fail.
    Held,
}

/// Is `port` bindable, and if not, by whom?
///
/// Both the wildcard and the loopback bind are tried, because a listener on
/// either blocks frynode: frynode binds `:<port>` (all interfaces) on purpose,
/// since remote clients dial a node's API to measure latency before selecting
/// it.
pub(crate) fn probe(port: u16) -> PortState {
    if bindable(("0.0.0.0", port)) && bindable(("127.0.0.1", port)) {
        return PortState::Free;
    }
    match owner_of(port) {
        Some((pid, image)) => PortState::HeldBy { pid, image },
        None => PortState::Held,
    }
}

/// Bind only — never listen. `std::net::TcpListener::bind` is bind + listen,
/// and listening on the wildcard address from fry-edge-miner.exe makes Windows
/// Defender Firewall ask the user to allow Fry Edge Miner on public and
/// private networks (c4 BUG LOOP 4). A bare bind reports a held port the same
/// way and asks nothing. The socket options match std's `bind`: SO_REUSEADDR
/// on Unix, so a closed listener's TIME_WAIT never reads as "held", and none
/// on Windows, where it would let the probe share a live listener's port.
fn bindable(addr: (&str, u16)) -> bool {
    let Ok(ip) = addr.0.parse::<std::net::IpAddr>() else {
        return false;
    };
    let socket = if ip.is_ipv4() {
        tokio::net::TcpSocket::new_v4()
    } else {
        tokio::net::TcpSocket::new_v6()
    };
    socket
        .and_then(|s| {
            #[cfg(unix)]
            s.set_reuseaddr(true)?;
            s.bind(std::net::SocketAddr::new(ip, addr.1))
        })
        .is_ok()
}

/// Resolve the listening owner of `port` through `netstat -ano` + `tasklist`.
///
/// `None` on any failure at all: an unresolved owner is a worse card message,
/// never a worse outcome.
#[cfg(target_os = "windows")]
fn owner_of(port: u16) -> Option<(u32, String)> {
    let out = command("netstat")
        .args(["-ano"])
        .output_bounded(OWNER_LOOKUP_TIMEOUT)
        .ok()?;
    let listing = String::from_utf8_lossy(&out.stdout).to_string();
    let pid = listening_pid(&listing, port)?;

    let out = command("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output_bounded(OWNER_LOOKUP_TIMEOUT)
        .ok()?;
    let image = first_image(&String::from_utf8_lossy(&out.stdout))?;
    Some((pid, image))
}

#[cfg(not(target_os = "windows"))]
fn owner_of(_port: u16) -> Option<(u32, String)> {
    // netstat -ano / tasklist are Windows-only; every shipped FEM is Windows.
    // On other platforms the probe still reports Free vs Held correctly.
    None
}

/// PURE: pull the PID of the LISTENING socket on `port` out of `netstat -ano`
/// output. Split out so the parsing is testable on any platform.
///
/// Rows look like:
///   TCP    0.0.0.0:8088    0.0.0.0:0    LISTENING    4312
/// Only LISTENING rows count — an outbound connection from an ephemeral port
/// that happens to equal 8088 is not holding it.
pub(crate) fn listening_pid(netstat_output: &str, port: u16) -> Option<u32> {
    let suffix = format!(":{port}");
    for line in netstat_output.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 5 || !cols[0].eq_ignore_ascii_case("TCP") {
            continue;
        }
        if !cols[3].eq_ignore_ascii_case("LISTENING") {
            continue;
        }
        if !cols[1].ends_with(&suffix) {
            continue;
        }
        if let Ok(pid) = cols[4].trim().parse::<u32>() {
            return Some(pid);
        }
    }
    None
}

/// PURE: the image name from a `tasklist /NH` row.
///
/// Rows look like:
///   python.exe                    4312 Console                 1     12,345 K
/// The image name may itself contain spaces, so the name is everything before
/// the first column that parses as the PID.
pub(crate) fn first_image(tasklist_output: &str) -> Option<String> {
    for line in tasklist_output.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("INFO:") {
            continue;
        }
        let cols: Vec<&str> = line.split_whitespace().collect();
        let pid_at = cols.iter().position(|c| c.parse::<u32>().is_ok())?;
        if pid_at == 0 {
            continue;
        }
        return Some(cols[..pid_at].join(" "));
    }
    None
}

/// The literal the supervisor matches to know this is not a fault to restart
/// from. Kept as a constant so the message and `AWAITING_MARKERS` cannot drift.
pub(crate) const PORT_HELD_MARKER: &str = "waiting for that program to release it";

/// PURE: what the card says about a held port.
pub(crate) fn conflict_reason(state: &PortState, port: u16) -> String {
    match state {
        PortState::HeldBy { pid, image } => format!(
            "frynode could not start: TCP port {port} is already in use by {image} (PID {pid}) — {PORT_HELD_MARKER}"
        ),
        _ => format!(
            "frynode could not start: TCP port {port} is already in use by another program — {PORT_HELD_MARKER}"
        ),
    }
}

#[cfg(test)]
#[path = "port_conflict_tests.rs"]
mod port_conflict_tests;

/// c4 BUG LOOP 4 (BL4-B): the probe binds, it never listens.
#[cfg(test)]
#[path = "port_probe_no_listen_tests.rs"]
mod port_probe_no_listen_tests;
