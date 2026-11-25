//! U6 RED->GREEN tests for FEM-DISCORD-AEM-900S-TIMEOUT.

use std::time::Duration;
use tokio::io::AsyncWriteExt;

const DOWNLOAD_RS: &str = include_str!("download.rs");

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn stalled_body_cleanup_removes_part_file_on_cancellation() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local test server");
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = tokio::io::AsyncReadExt::read(&mut sock, &mut buf).await;
            let _ = sock
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\n\r\n")
                .await;
            let _ = sock.flush().await;
            // Hold connection open with 0 body bytes sent so client stalls.
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });

    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("olostep-setup.exe");
    let part = dest.with_extension("part");
    let url = format!("http://{addr}/setup.exe");

    let res = tokio::time::timeout(
        Duration::from_millis(600),
        super::download::download_file(&url, &dest),
    )
    .await;

    server.abort();
    assert!(
        res.is_err(),
        "expected outer 600ms timeout to cancel stalled download"
    );
    assert!(
        !part.exists(),
        "RAII Drop guard must remove orphaned .part file ({}) when download is cancelled",
        part.display()
    );
}

#[test]
fn build_client_configures_connect_and_read_timeouts_and_http1_fallback() {
    let code = code_only(DOWNLOAD_RS);
    assert!(
        code.contains("connect_timeout(Duration::from_secs(15))"),
        "download.rs must configure connect_timeout(15s)"
    );
    assert!(
        code.contains("read_timeout(Duration::from_secs(30))"),
        "download.rs must configure read_timeout(30s)"
    );
    assert!(
        code.contains("http1_only()"),
        "download.rs must include an HTTP/1.1 fallback client for stalled HTTP/2 retries"
    );
}

#[tokio::test]
async fn retry_fallback_succeeds_after_initial_empty_close() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local test server");
    let addr = listener.local_addr().unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let server_attempts = Arc::clone(&attempts);

    let server = tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let n = server_attempts.fetch_add(1, Ordering::SeqCst) + 1;
            let mut buf = [0u8; 1024];
            let _ = tokio::io::AsyncReadExt::read(&mut sock, &mut buf).await;
            if n == 1 {
                // Attempt 1: send 200 OK with truncated body to trigger retry + HTTP/1.1 fallback.
                let _ = sock
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n")
                    .await;
                let _ = sock.shutdown().await;
            } else {
                let _ = sock
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello")
                    .await;
                let _ = sock.shutdown().await;
                break;
            }
        }
    });

    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("ok.bin");
    let url = format!("http://{addr}/ok.bin");

    super::download::download_file(&url, &dest)
        .await
        .expect("attempt 2 HTTP/1.1 fallback should succeed");

    let _ = server.await;
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
    assert!(!dest.with_extension("part").exists());
}
