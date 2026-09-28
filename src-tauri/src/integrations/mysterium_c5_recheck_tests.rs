//! c5 F7 pins for lens-1 survivors in the Mysterium missing-token state:
//! RC15-LE / RC15-RECHK (the 10-minute window) and RC16-A..G, L, M (the
//! credentials re-check when the window expires). The existing tests read
//! `health_check`'s source text and the pure helpers only, so a negated
//! verdict, a swapped re-arm/clear, a dropped `return`, an already-expired
//! re-arm or a broken `token_present` all passed. These drive the real
//! `health_check` against a local credentials endpoint on 127.0.0.1.

use super::*;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// `TOKEN_MISSING_SINCE` is process-wide, so these tests take turns. An
/// async mutex, because the turn is held across `health_check().await`.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Synthetic device key.
const KEY: &str = "FEM-C5PIN0000000000000000000000000000";

/// A credentials endpoint that answers every `GET /credentials/{KEY}` the
/// same way and counts them.
fn credentials_stub(status: &'static str, body: &'static str) -> (String, Arc<AtomicUsize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1");
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let reads = Arc::new(AtomicUsize::new(0));
    let counted = reads.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => request.extend_from_slice(&buf[..n]),
                }
            }
            if String::from_utf8_lossy(&request).starts_with(&format!("GET /credentials/{KEY} ")) {
                counted.fetch_add(1, Ordering::SeqCst);
            }
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (base_url, reads)
}

/// A MysteriumIntegration whose sdk_client is not running (a fresh
/// supervisor) and whose credentials come from `base_url`.
fn mysterium(base_url: String, dir: &std::path::Path) -> MysteriumIntegration {
    let config_dir = dir.join("config");
    let log_dir = dir.join("logs");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&log_dir).unwrap();
    let config = Arc::new(ConfigStore::new(config_dir, None));
    config
        .update(|c| c.miner_key = Some(KEY.to_string()))
        .unwrap();
    MysteriumIntegration {
        api_client: Arc::new(ApiClient::new(base_url, String::new())),
        config,
        supervisor: Arc::new(Mutex::new(Supervisor::new(log_dir.clone()))),
        log_dir,
    }
}

/// A missing-token state whose 10-minute window has already run out. `None`
/// only where the monotonic clock cannot reach that far back (a Windows host
/// booted less than ~10 minutes ago).
fn expired_state() -> Option<Instant> {
    Instant::now().checked_sub(TOKEN_RECHECK + Duration::from_secs(1))
}

fn setup_required(status: &HealthStatus) -> bool {
    matches!(status, HealthStatus::Unhealthy(r) if r == TOKEN_NOT_PROVISIONED_REASON)
}

fn not_running(status: &HealthStatus) -> bool {
    matches!(status, HealthStatus::Unhealthy(r)
        if r.starts_with("Mysterium SDK client process is not running"))
}

/// Runs `scenario` from an expired missing-token state against a stub
/// answering `status`/`body`, and resets the process-wide state afterwards.
async fn from_an_expired_window(
    status: &'static str,
    body: &'static str,
    scenario: impl AsyncFnOnce(&MysteriumIntegration, &AtomicUsize),
) {
    let _turn = SERIAL.lock().await;
    let Some(expired) = expired_state() else {
        eprintln!("skipped: this host's monotonic clock cannot reach 601 s back");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (base_url, reads) = credentials_stub(status, body);
    let m = mysterium(base_url, dir.path());
    *TOKEN_MISSING_SINCE.lock().unwrap() = Some(expired);
    scenario(&m, &reads).await;
    *TOKEN_MISSING_SINCE.lock().unwrap() = None;
}

/// Still no token after the window: SETUP REQUIRED again (RC16-G negates the
/// verdict, RC16-M drops the `return`, RC16-A inverts `token_present`), and
/// the window is re-armed from now, so the next tick answers from it without
/// another read (RC16-L clears instead of re-arming, RC16-E re-arms an
/// already-expired window).
#[tokio::test]
async fn a_token_still_missing_after_the_window_stays_setup_required_for_another_window() {
    from_an_expired_window(
        "200 OK",
        r#"{"miner_key":"FEM-C5PIN0000000000000000000000000000","mystnodes_user_token":""}"#,
        async |m: &MysteriumIntegration, reads: &AtomicUsize| {
            let first = m.health_check().await;
            assert!(setup_required(&first), "{first:?}");
            assert_eq!(
                reads.load(Ordering::SeqCst),
                1,
                "one credentials read on expiry"
            );
            let second = m.health_check().await;
            assert!(setup_required(&second), "{second:?}");
            assert_eq!(
                reads.load(Ordering::SeqCst),
                1,
                "the re-armed window answers the next tick without reading again"
            );
        },
    )
    .await;
}

/// The token arrived: the setup state ends and stays ended, so the card
/// shows the real not-running state and the supervisor restarts into a spawn
/// (RC16-A/B/D misread a present token, RC16-G negates the verdict, RC16-L
/// re-arms instead of clearing).
#[tokio::test]
async fn a_token_that_arrived_ends_the_setup_state() {
    from_an_expired_window(
        "200 OK",
        r#"{"miner_key":"FEM-C5PIN0000000000000000000000000000","mystnodes_user_token":"c5-synthetic-token"}"#,
        async |m: &MysteriumIntegration, reads: &AtomicUsize| {
            let first = m.health_check().await;
            assert!(not_running(&first), "{first:?}");
            assert_eq!(reads.load(Ordering::SeqCst), 1);
            let second = m.health_check().await;
            assert!(not_running(&second), "{second:?}");
            assert_eq!(
                reads.load(Ordering::SeqCst),
                1,
                "a cleared state reads nothing more"
            );
        },
    )
    .await;
}

/// An unreadable credentials read keeps the last verdict: still SETUP
/// REQUIRED, never an unproven "token present" (RC16-C).
#[tokio::test]
async fn an_unreadable_credentials_read_keeps_the_setup_state() {
    from_an_expired_window(
        "500 Internal Server Error",
        r#"{"error":"unavailable"}"#,
        async |m: &MysteriumIntegration, reads: &AtomicUsize| {
            let status = m.health_check().await;
            assert!(setup_required(&status), "{status:?}");
            assert_eq!(reads.load(Ordering::SeqCst), 1);
        },
    )
    .await;
}

/// The window is exactly ten minutes, as the reason text promises: still
/// missing at 599 s (RC15-RECHK shortens it), over at 600 s (RC15-LE keeps
/// it one tick longer). Built forward from `t0`, so no clock underflow.
#[test]
fn the_window_is_exactly_ten_minutes() {
    let t0 = Instant::now();
    assert!(token_missing_recently(
        Some(t0),
        t0 + Duration::from_secs(599)
    ));
    assert!(!token_missing_recently(
        Some(t0),
        t0 + Duration::from_secs(600)
    ));
}
