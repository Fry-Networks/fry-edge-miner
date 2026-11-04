//! The client authenticates only with its own per-device token. It registers
//! when it holds none, or when the server rejected the one it holds, and never
//! falls back to a shared or environment-supplied token.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::api::client::ApiClient;
use crate::config::store::ConfigStore;

const KEY: &str = "FEM-0123456789ABCDEF0123456789ABCDEF";
const INSTALL: &str = "install-test-1";
const OLD_TOKEN: &str = "fem_old_test_token";
const NEW_TOKEN: &str = "fem_new_test_token";

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Minimal scripted HTTP stub: records `METHOD path` for every request and
/// answers each with the next scripted (status, body), repeating the last.
struct Stub {
    base: String,
    seen: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
}

impl Stub {
    fn start(script: Vec<(u16, String)>) -> Stub {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind stub");
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (seen2, stop2) = (seen.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut n = 0usize;
            while !stop2.load(Ordering::Relaxed) {
                let Ok((mut s, _)) = listener.accept() else {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                };
                s.set_nonblocking(false).unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 2048];
                while let Ok(k) = s.read(&mut chunk) {
                    if k == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..k]);
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..p]).to_lowercase();
                        let len = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if buf.len() >= p + 4 + len {
                            break;
                        }
                    }
                }
                let text = String::from_utf8_lossy(&buf).to_string();
                let mut parts = text.lines().next().unwrap_or("").split_whitespace();
                let line = format!(
                    "{} {}",
                    parts.next().unwrap_or(""),
                    parts.next().unwrap_or("")
                );
                seen2.lock().unwrap().push(line);
                let (status, body) = script
                    .get(n)
                    .or_else(|| script.last())
                    .cloned()
                    .unwrap_or((200, "{}".to_string()));
                n += 1;
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes());
            }
        });
        Stub { base, seen, stop }
    }

    fn installs(&self) -> usize {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|l| **l == format!("POST /installations/{KEY}/installations/{INSTALL}"))
            .count()
    }

    fn key_posts(&self) -> usize {
        let prefix = format!("POST /installations/{KEY}/installations/");
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.starts_with(&prefix))
            .count()
    }

    fn total(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn store(tag: &str, token: Option<&str>) -> Arc<ConfigStore> {
    let dir = std::env::temp_dir().join(format!("fem-token-split-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cs = Arc::new(ConfigStore::new(dir, None));
    cs.update(|c| {
        c.miner_key = Some(KEY.to_string());
        c.install_id = Some(INSTALL.to_string());
        c.device_token = token.map(str::to_string);
    })
    .unwrap();
    cs
}

fn reg_ok() -> (u16, String) {
    (
        200,
        format!(r#"{{"status":"ok","device_token":"{NEW_TOKEN}"}}"#),
    )
}

fn lease_ok() -> (u16, String) {
    (200, r#"{"granted":true}"#.to_string())
}

#[tokio::test]
async fn startup_with_a_stored_token_makes_no_registration_call() {
    let stub = Stub::start(vec![reg_ok()]);
    let cs = store("s2", Some(OLD_TOKEN));
    let api = Arc::new(ApiClient::new(stub.base.clone(), OLD_TOKEN.to_string()));
    super::device::attempt_device_token_migration(&cs, &api).await;
    assert_eq!(stub.installs(), 0, "seen: {:?}", stub.seen.lock().unwrap());
    assert_eq!(cs.get().device_token.as_deref(), Some(OLD_TOKEN));
}

#[tokio::test]
async fn a_version_change_with_a_stored_token_does_not_register_or_rotate_it() {
    let stub = Stub::start(vec![lease_ok()]);
    let cs = store("s3", Some(OLD_TOKEN));
    let api = Arc::new(ApiClient::new(stub.base.clone(), OLD_TOKEN.to_string()));
    super::device::attempt_version_change_heartbeat(&cs, &api, "99.0.0").await;
    assert_eq!(stub.installs(), 0, "seen: {:?}", stub.seen.lock().unwrap());
    assert_eq!(cs.get().device_token.as_deref(), Some(OLD_TOKEN));
    // Recorded only because the authenticated lease call succeeded.
    assert_eq!(cs.get().last_reported_version.as_deref(), Some("99.0.0"));
}

#[tokio::test]
async fn a_version_change_is_not_recorded_when_the_authenticated_call_fails() {
    let stub = Stub::start(vec![(500, "{}".to_string())]);
    let cs = store("s3f", Some(OLD_TOKEN));
    let api = Arc::new(ApiClient::new(stub.base.clone(), OLD_TOKEN.to_string()));
    super::device::attempt_version_change_heartbeat(&cs, &api, "99.0.0").await;
    assert_eq!(stub.installs(), 0);
    assert_eq!(cs.get().last_reported_version, None);
}

#[tokio::test]
async fn a_rejected_stored_token_is_replaced_with_exactly_one_registration() {
    for status in [401u16, 403] {
        // The first call is rejected by the server with the status under test;
        // the second answer is the registration.
        let stub = Stub::start(vec![(status, "{}".to_string()), reg_ok()]);
        let cs = store(&format!("rej{status}"), Some(OLD_TOKEN));
        let api = Arc::new(ApiClient::new(stub.base.clone(), OLD_TOKEN.to_string()));
        let err = api
            .put_json(&format!("/PoC/{KEY}/hardware"), &serde_json::json!({}))
            .await
            .expect_err("the stub rejects this call");
        let seen = super::device::api_error_status(&err);
        assert_eq!(seen, Some(status));
        let now = std::time::Instant::now();
        assert!(super::device::should_attempt_rejection_recovery(
            seen,
            true,
            None,
            now,
            super::device::TOKEN_RECOVERY_COOLDOWN
        ));
        let ok = super::device::attempt_token_recovery(&cs, &api, Some(OLD_TOKEN)).await;
        assert!(ok, "status {status}");
        assert_eq!(stub.installs(), 1, "status {status}");
        assert_eq!(cs.get().device_token.as_deref(), Some(NEW_TOKEN));
    }
}

#[tokio::test]
async fn a_stale_rejection_does_not_rotate_a_fresher_token() {
    let stub = Stub::start(vec![reg_ok()]);
    // The 401 was observed against OLD_TOKEN, but a newer token is stored now.
    let cs = store("stale", Some("fem_fresh_test_token"));
    let api = Arc::new(ApiClient::new(
        stub.base.clone(),
        "fem_fresh_test_token".to_string(),
    ));
    let ok = super::device::attempt_token_recovery(&cs, &api, Some(OLD_TOKEN)).await;
    assert!(!ok);
    assert_eq!(stub.total(), 0, "seen: {:?}", stub.seen.lock().unwrap());
    assert_eq!(
        cs.get().device_token.as_deref(),
        Some("fem_fresh_test_token")
    );
}

#[tokio::test]
async fn concurrent_registration_hooks_make_exactly_one_post_and_keep_its_token() {
    let stub = Stub::start(vec![
        reg_ok(),
        (
            200,
            r#"{"status":"ok","device_token":"fem_second_minted"}"#.to_string(),
        ),
    ]);
    let cs = store("conc", None);
    cs.update(|c| c.install_id = None).unwrap();
    let api = Arc::new(ApiClient::new(stub.base.clone(), String::new()));
    let (a, b, c) = tokio::join!(
        super::device::attempt_registration_completion(&cs, &api),
        super::device::attempt_registration_completion(&cs, &api),
        super::device::attempt_device_token_migration(&cs, &api),
    );
    let _ = (a, b, c);
    assert_eq!(stub.key_posts(), 1, "seen: {:?}", stub.seen.lock().unwrap());
    assert_eq!(cs.get().device_token.as_deref(), Some(NEW_TOKEN));
    assert!(cs.get().install_id.is_some());
}

#[tokio::test]
async fn a_held_token_is_kept_when_only_the_install_id_is_missing() {
    let stub = Stub::start(vec![reg_ok()]);
    let cs = store("heldtoken", Some(OLD_TOKEN));
    cs.update(|c| {
        c.install_id = None;
        c.pending_install_id = Some("pending-1".to_string());
    })
    .unwrap();
    let api = Arc::new(ApiClient::new(stub.base.clone(), OLD_TOKEN.to_string()));
    assert!(super::device::attempt_registration_completion(&cs, &api).await);
    assert_eq!(stub.total(), 0);
    assert_eq!(cs.get().device_token.as_deref(), Some(OLD_TOKEN));
    assert_eq!(cs.get().install_id.as_deref(), Some("pending-1"));
}

#[test]
fn no_baked_or_env_token_is_ever_used() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("FEM_API_TOKEN", "env-token-for-test");
    let cfg = crate::config::FemConfig::default();
    let from_json: crate::config::FemConfig = serde_json::from_str("{}").unwrap();
    std::env::remove_var("FEM_API_TOKEN");
    assert_eq!(cfg.api_token, "");
    assert_eq!(cfg.effective_api_token(), "");
    assert_eq!(from_json.effective_api_token(), "");
}

#[test]
fn the_config_source_does_not_read_the_shared_token() {
    let src = include_str!("../config/mod.rs");
    let name = format!("FEM_API{}TOKEN", '_');
    assert!(!src.contains(&format!("var(\"{name}\")")));
    assert!(!src.contains(&format!("option_env!(\"{name}\")")));
}

#[test]
fn a_401_or_403_rejection_gates_one_recovery_per_cooldown() {
    use super::device::{should_attempt_rejection_recovery as gate, TOKEN_RECOVERY_COOLDOWN as CD};
    let now = std::time::Instant::now();
    for status in [401u16, 403] {
        assert!(
            gate(Some(status), true, None, now, CD),
            "{status} with a token"
        );
        assert!(
            !gate(Some(status), true, Some(now), now, CD),
            "{status} inside cooldown"
        );
        assert!(
            !gate(Some(status), false, None, now, CD),
            "{status} without a token"
        );
    }
    assert!(!gate(Some(500), true, None, now, CD));
    assert!(!gate(None, true, None, now, CD));
}

fn heartbeat() -> crate::api::types::InstallationHeartbeat {
    crate::api::types::InstallationHeartbeat {
        miner_key: KEY.to_string(),
        install_id: INSTALL.to_string(),
        miner_code: Some("FEM".to_string()),
        software_version_installed: None,
        poc_version_installed: None,
        hostname: None,
        os: None,
        is_installed: Some(true),
        device_name: None,
    }
}

#[tokio::test]
async fn a_failing_server_gets_one_attempt_plus_three_retries_then_gives_up() {
    let stub = Stub::start(vec![(503, "{}".to_string())]);
    let api = ApiClient::new(stub.base.clone(), String::new());
    let r = super::device::register_with_backoff(
        &api,
        &heartbeat(),
        std::time::Duration::from_millis(1),
    )
    .await;
    assert!(r.is_err());
    assert_eq!(stub.total(), 4);
}

#[tokio::test]
async fn a_rejection_is_not_retried_by_the_backoff() {
    let stub = Stub::start(vec![(401, "{}".to_string())]);
    let api = ApiClient::new(stub.base.clone(), String::new());
    let r = super::device::register_with_backoff(
        &api,
        &heartbeat(),
        std::time::Duration::from_millis(1),
    )
    .await;
    assert!(r.is_err());
    assert_eq!(stub.total(), 1);
}

#[tokio::test]
async fn an_unreachable_server_fails_gracefully_with_no_fallback_token() {
    let dead = {
        let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        format!("http://{}", l.local_addr().unwrap())
    };
    let cs = store("dead", None);
    let api = Arc::new(ApiClient::new(dead, cs.get().effective_api_token()));
    let r = super::device::register_with_backoff(
        &api,
        &heartbeat(),
        std::time::Duration::from_millis(1),
    )
    .await;
    assert!(r.is_err());
    assert_eq!(cs.get().device_token, None);
    assert_eq!(cs.get().effective_api_token(), "");
}

#[tokio::test]
async fn a_device_with_no_token_registers_once_and_persists_it() {
    let stub = Stub::start(vec![reg_ok(), lease_ok()]);
    let cs = store("notoken", None);
    let api = Arc::new(ApiClient::new(stub.base.clone(), String::new()));
    super::device::attempt_version_change_heartbeat(&cs, &api, "99.0.0").await;
    super::device::attempt_device_token_migration(&cs, &api).await;
    assert_eq!(stub.installs(), 1, "seen: {:?}", stub.seen.lock().unwrap());
    assert_eq!(cs.get().device_token.as_deref(), Some(NEW_TOKEN));
}
