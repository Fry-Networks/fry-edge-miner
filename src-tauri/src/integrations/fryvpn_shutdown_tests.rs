//! B9 — deregister-on-shutdown.
//!
//! Two halves: FEM must stop quoting frynode's own shutdown log as the reason
//! the node "is not running", and a user who DISABLES the node must actually
//! get it deregistered instead of leaving its record and its 200_000 µALGO box
//! MBR stranded on chain.

use super::*;

/// The §6 B9 verbatim, as it appears in `fryvpn_stderr.log`. Go's stdlib
/// logger writes to stderr, so an orderly stop lands in the very file FEM
/// tails for a crash reason.
const SHUTDOWN_LOG: &str = "\
2026/09/15 02:18:04 shutting down...
2026/09/15 02:18:04 warning: failed to deregister node: deregister node on-chain: HTTP 400: {\"message\":\"TransactionPool.Remember: transaction ABC: overspend (account ZEFA3HDWNBEOIYQ6O6SQNY3NGKDI76D2L237CZWDW7K33ADLRVIPAKBTKQ, data {AccountBaseData:{Status:Offline MicroAlgos:0.0A}})\"}
2026/09/15 02:18:05 shutdown complete";

#[test]
fn a_shutdown_log_is_not_reported_as_the_failure_reason() {
    let reason = not_running_reason_from_stderr(SHUTDOWN_LOG);
    assert_eq!(
        reason, "frynode process is not running",
        "a node that stopped on purpose must not be described by its own \
         shutdown sequence"
    );
    assert!(
        !reason.contains("overspend"),
        "the chain's raw rejection must never reach the card: {reason}"
    );
    assert!(
        !reason.contains("deregister"),
        "a shutdown-path error is not why the process is gone: {reason}"
    );
}

/// The lines the fixed frynode writes when it declines to deregister are
/// informational, not failures, and must be filtered too.
#[test]
fn the_deregister_skip_lines_are_filtered_as_well() {
    let log = "\
2026/09/15 02:18:04 shutting down...
2026/09/15 02:18:04 shutdown: node was never registered on-chain; nothing to deregister
2026/09/15 02:18:05 shutdown complete";
    assert_eq!(
        not_running_reason_from_stderr(log),
        "frynode process is not running"
    );
}

#[test]
fn a_real_crash_is_still_quoted() {
    let log = "2026/09/15 02:18:04 failed to load config: REGION is required";
    let reason = not_running_reason_from_stderr(log);
    assert!(
        reason.contains("REGION is required"),
        "a genuine startup failure must still be shown: {reason}"
    );
}

/// A crash that happens to be followed by shutdown lines must still surface
/// the crash — filtering removes the noise, not the diagnosis.
#[test]
fn a_crash_followed_by_a_shutdown_sequence_still_surfaces_the_crash() {
    let log = "\
2026/09/15 02:18:03 panic: listen tcp :8088: bind: address already in use
2026/09/15 02:18:04 shutting down...
2026/09/15 02:18:05 shutdown complete";
    let reason = not_running_reason_from_stderr(log);
    assert!(
        reason.contains("address already in use"),
        "the real cause must survive the filter: {reason}"
    );
}

#[test]
fn an_empty_stderr_is_the_plain_reason() {
    assert_eq!(
        not_running_reason_from_stderr(""),
        "frynode process is not running"
    );
}

/// A filtered shutdown reason must not be mistaken for an awaiting-user state:
/// the supervisor should still be free to restart a node that really died.
#[test]
fn the_filtered_reason_is_still_a_restartable_fault() {
    assert!(!crate::integrations::awaits_user_action(
        &not_running_reason_from_stderr(SHUTDOWN_LOG)
    ));
}

#[tokio::test]
async fn graceful_shutdown_request_reports_failure_when_nothing_listens() {
    // Port 1 is reserved and never bound; the call must give up quickly
    // rather than delay a disable the user asked for.
    let started = std::time::Instant::now();
    assert!(!request_graceful_shutdown(1, Duration::from_millis(500)).await);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "a dead port must not hold up the stop"
    );
}

#[tokio::test]
async fn graceful_shutdown_request_reports_success_on_204() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = sock.read(&mut buf).await;
        let _ = sock
            .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
            .await;
        let _ = sock.flush().await;
    });

    assert!(request_graceful_shutdown(port, Duration::from_secs(5)).await);
    let _ = server.await;
}

/// The trait method is DEFAULTED, so no other integration changed behaviour.
#[tokio::test]
async fn the_default_stop_for_disable_delegates_to_stop() {
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingStop {
        stops: AtomicUsize,
    }

    #[async_trait]
    impl Integration for CountingStop {
        fn id(&self) -> &str {
            "counting"
        }
        fn display_name(&self) -> &str {
            "Counting"
        }
        async fn install(&self) -> Result<()> {
            Ok(())
        }
        async fn start(&self) -> Result<()> {
            Ok(())
        }
        async fn stop(&self) -> Result<()> {
            self.stops.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn health_check(&self) -> HealthStatus {
            HealthStatus::Healthy
        }
        async fn check_update(&self) -> Result<Option<String>> {
            Ok(None)
        }
    }

    let c = CountingStop {
        stops: AtomicUsize::new(0),
    };
    c.stop_for_disable().await.unwrap();
    assert_eq!(
        c.stops.load(Ordering::SeqCst),
        1,
        "an integration that does not override stop_for_disable must be stopped \
         exactly as before"
    );
}
