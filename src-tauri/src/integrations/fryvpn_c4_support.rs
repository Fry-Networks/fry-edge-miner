//! Test support shared by the continuation-#4 fryDVPN tests (FAIL-12, FAIL-2):
//! a loopback decoy standing in for hardwareapi and algod, and a runner that
//! executes each scenario in a CHILD re-execution of this test binary.
//!
//! Why a child process: the algod overrides are process-global environment
//! variables that `tests::the_algod_*` mutate under a lock private to
//! `mod tests`, and `PARKED_FUNDING_REASON` is a process-global static. A
//! scenario sharing the test process with them could read another test's
//! endpoint — including the real mainnet default — or another scenario's
//! park. The child runs exactly one test and owns its whole environment.
//!
//! Nothing here reaches a real network: every URL the product code is handed
//! points at 127.0.0.1, proxies are removed from the child's environment, and
//! frynode is a name that does not exist (no firewall rule is touched and
//! nothing is launched) unless a scenario supplies its own decoy.

use super::*;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};

/// Synthetic, checksum-valid Algorand address — not a real wallet.
pub(super) const ADDR: &str = "YTC4NR6IZHFMXTGNZ3H5BUOS2PKNLVWX3DM5VW643XPN7YHB4LRUS2CHMA";
pub(super) const MINER_KEY: &str = "FEM-C4DECOYKEY";
/// A decoy algod token: LocalNet's role, none of its value.
pub(super) const TOKEN: &str = "c4decoy-algod-token";
/// A non-default registry app id, so a hard-coded mainnet id cannot pass.
pub(super) const APP_ID: &str = "1001";
pub(super) const MIN_BALANCE: u64 = 100_000;
/// A frynode that does not exist. A relative name skips the firewall rule, and
/// the spawn then fails with "Failed to spawn frynode" — which is how a
/// scenario sees that the funding gate let the start through.
pub(super) const NO_SUCH_FRYNODE: &str = "fem-c4-no-such-frynode";
pub(super) const SCENARIO_VAR: &str = "FEM_C4_SCENARIO";
pub(super) const DONE: &str = "FEM-C4-SCENARIO-DONE";

/// Removed from the child so nothing another test is midway through setting,
/// and no proxy, can leak into a scenario.
const SCRUBBED_ENV: [&str; 14] = [
    "FRYNODE_ALGOD_SERVER",
    "FRYNODE_ALGOD_PORT",
    "FRYNODE_ALGOD_TOKEN",
    "FRYNODE_REGISTRY_APP_ID",
    "FRYNODE_BIN",
    "FRYNODE_REGION",
    "FRYNODE_CAPACITY_MBPS",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "NODE_MNEMONIC",
];

pub(super) fn mnemonic() -> String {
    ["decoy"; 25].join(" ")
}

pub(super) fn account_path() -> String {
    format!("/v2/accounts/{ADDR}")
}

pub(super) fn box_path() -> String {
    format!("/v2/applications/{APP_ID}/box?name=addr:{ADDR}")
}

/// Run `scenario` in a child re-execution of this test binary. Fails with the
/// child's own output — its assertion message included — when the scenario
/// failed, and separately when the child did not run exactly one test, so a
/// drifted filter can never pass as an empty run.
pub(super) fn run_scenario(module: &str, scenario: &str) {
    let child_test = format!(
        "{}::child",
        module.split_once("::").map_or(module, |(_, rest)| rest)
    );
    let mut cmd =
        std::process::Command::new(std::env::current_exe().expect("path of this test binary"));
    cmd.args([
        child_test.as_str(),
        "--exact",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ])
    .env(SCENARIO_VAR, scenario)
    .env("NO_PROXY", "127.0.0.1,localhost")
    .env("no_proxy", "127.0.0.1,localhost");
    for key in SCRUBBED_ENV {
        cmd.env_remove(key);
    }
    let out = cmd.output().expect("re-execute this test binary");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("running 1 test"),
        "harness: `{child_test}` did not run exactly one test for scenario `{scenario}`:\n{text}"
    );
    assert!(
        out.status.success() && text.contains(&format!("{DONE} {scenario}")),
        "scenario `{scenario}` failed in its child process:\n{text}"
    );
}

/// `run_scenario` for a scenario that binds or probes frynode's API port
/// (8088). Such children are serialized, so one's `/health` decoy can never be
/// mistaken by another for a program holding the port.
pub(super) fn run_scenario_on_frynode_port(module: &str, scenario: &str) {
    static FRYNODE_PORT: Mutex<()> = Mutex::new(());
    let _one_at_a_time = FRYNODE_PORT.lock().unwrap_or_else(|e| e.into_inner());
    run_scenario(module, scenario);
}

/// What algod answers for the device account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Account {
    Balance,
    /// A rate-limit / gateway page where JSON was expected.
    Html503,
}

/// What algod answers for this node's NodeRegistry box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RegistryBox {
    /// The node is registered: algod returns the box with its `value`.
    Present,
    /// algod's own answer for a node that is not registered.
    Absent,
    /// A 200 that is not algod's box JSON at all.
    Html200,
    /// A 200 JSON without the box `value`.
    JsonWithoutValue,
    Html503,
    /// A 404 that is NOT algod's "box not found" (a proxy's page).
    Html404,
    /// The connection closes before any response: a transport failure.
    Dropped,
}

#[derive(Debug, Clone)]
pub(super) struct World {
    pub amount: u64,
    pub min_balance: u64,
    pub account: Account,
    pub registry_box: RegistryBox,
    /// Answer every `/v2` route with algod's 401 unless the token header
    /// matches — how a token-protected algod such as LocalNet behaves.
    pub require_token: bool,
    /// Whether hardwareapi releases the device mnemonic (it does not when the
    /// encrypted blob fails to decrypt).
    pub mnemonic_released: bool,
}

impl World {
    pub fn new(amount: u64, registry_box: RegistryBox) -> Self {
        Self {
            amount,
            min_balance: MIN_BALANCE,
            account: Account::Balance,
            registry_box,
            require_token: false,
            mnemonic_released: true,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct Request {
    pub method: String,
    pub target: String,
    headers: HashMap<String, String>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

pub(super) enum Reply {
    Http(u16, &'static str, String),
    /// Close the connection without a byte.
    Dropped,
}

fn json(status: u16, body: impl Into<String>) -> Reply {
    Reply::Http(status, "application/json", body.into())
}

fn html(status: u16, body: &str) -> Reply {
    Reply::Http(status, "text/html", body.to_string())
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

/// `%3A` and friends back to their characters, so a correct client that
/// percent-encodes the box name is routed exactly like one that does not.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn read_request(conn: &mut TcpStream) -> Option<Request> {
    conn.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = conn.read(&mut chunk).ok()?;
        if n == 0 || buf.len() > 64 * 1024 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    let mut lines = text.split("\r\n");
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_string();
    let target = percent_decode(first.next()?);
    let headers = lines
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    Some(Request {
        method,
        target,
        headers,
    })
}

/// A loopback HTTP decoy that records every request it receives.
pub(super) struct Decoy {
    pub port: u16,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Decoy {
    pub fn serve(
        addr: &str,
        route: impl Fn(&Request) -> Reply + Send + 'static,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let port = listener.local_addr()?.port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (log, halt) = (requests.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            for conn in listener.incoming() {
                if halt.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut conn) = conn else { continue };
                let Some(req) = read_request(&mut conn) else {
                    continue;
                };
                log.lock().unwrap().push(req.clone());
                if let Reply::Http(status, ctype, body) = route(&req) {
                    let head = format!(
                        "HTTP/1.1 {status} {}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        reason_phrase(status),
                        body.len()
                    );
                    let _ = conn.write_all(head.as_bytes());
                    let _ = conn.write_all(body.as_bytes());
                    let _ = conn.flush();
                }
            }
        });
        Ok(Self {
            port,
            requests,
            stop,
            thread: Some(thread),
        })
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }

    pub fn requests_to(&self, target: &str) -> Vec<Request> {
        self.requests()
            .into_iter()
            .filter(|r| r.target == target)
            .collect()
    }
}

impl Drop for Decoy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocking accept so the thread sees the flag and exits.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// hardwareapi's credentials lookup and algod's account and registry-box
/// reads, answered from `world` at the moment each request arrives.
fn route(world: Arc<Mutex<World>>) -> impl Fn(&Request) -> Reply + Send + 'static {
    move |req| {
        let w = world.lock().unwrap().clone();
        let target = req.target.as_str();
        if target == format!("/credentials/{MINER_KEY}") {
            return json(
                200,
                serde_json::json!({
                    "miner_key": MINER_KEY,
                    "algo_address": ADDR,
                    "algo_mnemonic": w.mnemonic_released.then(mnemonic),
                })
                .to_string(),
            );
        }
        if target.starts_with("/v2/")
            && w.require_token
            && req.header("X-Algo-API-Token") != Some(TOKEN)
        {
            return json(401, r#"{"message":"Invalid API Token"}"#);
        }
        if target == account_path() {
            return match w.account {
                Account::Balance => json(
                    200,
                    serde_json::json!({
                        "address": ADDR,
                        "amount": w.amount,
                        "min-balance": w.min_balance,
                        "round": 1,
                    })
                    .to_string(),
                ),
                Account::Html503 => html(503, "<html><body>quota exceeded</body></html>"),
            };
        }
        if target == box_path() {
            return match w.registry_box {
                RegistryBox::Present => json(
                    200,
                    r#"{"name":"xsTEyw==","round":1,"value":"AAAAAAAAAAE="}"#,
                ),
                RegistryBox::Absent => json(404, r#"{"message":"box not found"}"#),
                RegistryBox::Html200 => html(200, "<!DOCTYPE html><html>maintenance</html>"),
                RegistryBox::JsonWithoutValue => json(200, r#"{"message":"ok"}"#),
                RegistryBox::Html503 => html(503, "<html>upstream unavailable</html>"),
                RegistryBox::Html404 => html(404, "<html>Not Found</html>"),
                RegistryBox::Dropped => Reply::Dropped,
            };
        }
        json(404, r#"{"message":"decoy: no such route"}"#)
    }
}

/// How the child points FEM at the decoy algod.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Endpoint {
    /// `FRYNODE_ALGOD_SERVER` carries the port itself.
    PortInline,
    /// `FRYNODE_ALGOD_SERVER` is a bare host and `FRYNODE_ALGOD_PORT` names
    /// the port — frynode's own form, which it joins as `server:port`.
    PortOverride,
}

struct LogSink(Arc<Mutex<Vec<u8>>>);

impl Write for LogSink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        // Echoed too, so a failing child's output shows what the product logged.
        let _ = std::io::stderr().write_all(b);
        Ok(b.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn capture_logs() -> Arc<Mutex<Vec<u8>>> {
    static LOGS: std::sync::OnceLock<Arc<Mutex<Vec<u8>>>> = std::sync::OnceLock::new();
    LOGS.get_or_init(|| {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let sink = buf.clone();
        let _ = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .with_writer(move || LogSink(sink.clone()))
            .try_init();
        buf
    })
    .clone()
}

/// One scenario's world, decoy and integration. CHILD PROCESS ONLY: it takes
/// over this process's environment.
pub(super) struct Scene {
    pub world: Arc<Mutex<World>>,
    pub algod: Decoy,
    pub integ: FryVpnIntegration,
    pub rt: tokio::runtime::Runtime,
    logs: Arc<Mutex<Vec<u8>>>,
    pub tmp: tempfile::TempDir,
}

impl Scene {
    pub fn new(world: World, endpoint: Endpoint, token: Option<&str>) -> Self {
        assert!(
            std::env::var(SCENARIO_VAR).is_ok(),
            "harness: a Scene may only be built inside a scenario child process"
        );
        let logs = capture_logs();
        let tmp = tempfile::tempdir().expect("scenario tempdir");
        // Keep every directory the start path resolves inside the tempdir.
        std::env::set_var("XDG_DATA_HOME", tmp.path().join("data"));
        let storage = tmp.path().join("storage");
        std::fs::create_dir_all(&storage).expect("storage dir");
        let _ = crate::integrations::download::init_storage_root(Some(&storage.to_string_lossy()));

        let world = Arc::new(Mutex::new(world));
        let algod = Decoy::serve("127.0.0.1:0", route(world.clone())).expect("bind the decoy");
        match endpoint {
            Endpoint::PortInline => {
                std::env::set_var("FRYNODE_ALGOD_SERVER", algod.url());
                std::env::remove_var("FRYNODE_ALGOD_PORT");
            }
            Endpoint::PortOverride => {
                std::env::set_var("FRYNODE_ALGOD_SERVER", "http://127.0.0.1");
                std::env::set_var("FRYNODE_ALGOD_PORT", algod.port.to_string());
            }
        }
        match token {
            Some(t) => std::env::set_var("FRYNODE_ALGOD_TOKEN", t),
            None => std::env::remove_var("FRYNODE_ALGOD_TOKEN"),
        }
        std::env::set_var("FRYNODE_REGISTRY_APP_ID", APP_ID);
        std::env::set_var("FRYNODE_BIN", NO_SUCH_FRYNODE);

        let config_dir = tmp.path().join("config");
        std::fs::create_dir_all(&config_dir).expect("config dir");
        let config = crate::config::store::ConfigStore::new(config_dir, None);
        config
            .update(|c| c.miner_key = Some(MINER_KEY.to_string()))
            .expect("seed the decoy miner key");
        let logs_dir = tmp.path().join("logs");
        let integ = FryVpnIntegration {
            config: Arc::new(config),
            api_client: Arc::new(crate::api::client::ApiClient::new(
                algod.url(),
                String::new(),
            )),
            supervisor: Arc::new(Mutex::new(crate::supervisor::Supervisor::new(
                logs_dir.clone(),
            ))),
            log_dir: logs_dir,
        };
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime");
        Self {
            world,
            algod,
            integ,
            rt,
            logs,
            tmp,
        }
    }

    pub fn block_on<F: std::future::Future>(&self, f: F) -> F::Output {
        self.rt.block_on(f)
    }

    pub fn set_world(&self, f: impl FnOnce(&mut World)) {
        f(&mut self.world.lock().unwrap());
    }

    pub fn parked(&self) -> Option<String> {
        PARKED_FUNDING_REASON.lock().unwrap().clone()
    }

    pub fn reads(&self, target: &str) -> usize {
        self.algod.requests_to(target).len()
    }

    /// Log lines, from anywhere in this child, that contain `needle`.
    pub fn log_lines(&self, needle: &str) -> Vec<String> {
        String::from_utf8_lossy(&self.logs.lock().unwrap())
            .lines()
            .filter(|l| l.contains(needle))
            .map(str::to_string)
            .collect()
    }

    /// Only reads ever reach the decoy: no transaction was submitted.
    pub fn assert_nothing_submitted(&self) {
        let all = self.algod.requests();
        assert!(
            all.iter()
                .all(|r| r.method == "GET" && !r.target.starts_with("/v2/transactions")),
            "something other than a read reached algod: {all:?}"
        );
    }

    /// The start reached frynode's spawn: the gate let it through.
    pub fn spawn_attempted(started: &anyhow::Result<()>) -> bool {
        matches!(started, Err(e) if e.to_string().contains("Failed to spawn frynode"))
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        // Clears the park and kills any decoy frynode this scene launched.
        let _ = self.rt.block_on(self.integ.stop());
    }
}

/// A frynode stand-in that stays alive whatever flags it is given. Unix only:
/// a script found on PATH under a bare name, so the start path's firewall step
/// (absolute paths only) is still skipped. It exits on its own within a
/// minute even if a failing scenario never stops it.
#[cfg(unix)]
pub(super) fn install_decoy_frynode(scene: &Scene) {
    use std::os::unix::fs::PermissionsExt;
    let bin = scene.tmp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("decoy bin dir");
    let script = bin.join("fem-c4-decoy-frynode");
    std::fs::write(&script, "#!/bin/sh\nexec sleep 60\n").expect("write the decoy frynode");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .expect("make the decoy executable");
    let path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{path}", bin.display()));
    std::env::set_var("FRYNODE_BIN", "fem-c4-decoy-frynode");
}

/// frynode's local `/health`, answering healthy and registered — what the real
/// node reports once it has found its box on chain.
pub(super) fn frynode_health_decoy() -> Decoy {
    let route = |req: &Request| {
        if req.target == "/health" {
            json(200, r#"{"status":"healthy","registered":true}"#)
        } else {
            json(404, "{}")
        }
    };
    for _ in 0..50 {
        if let Ok(decoy) = Decoy::serve(&format!("127.0.0.1:{FRYNODE_API_PORT}"), route) {
            return decoy;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!(
        "harness: port {FRYNODE_API_PORT} stayed busy; this scenario needs it for frynode's \
         /health decoy"
    );
}

/// The not-running branch of `health_check` probes frynode's API port and, on
/// Windows, kills an UNTRACKED frynode.exe holding it. A scenario that reaches
/// that branch first proves the port is free, so it can never touch a real
/// node on the machine running the tests.
pub(super) fn require_frynode_port_free() {
    #[cfg(target_os = "windows")]
    {
        fn bindable(addr: (&str, u16)) -> bool {
            TcpListener::bind(addr).is_ok()
        }
        let free = (0..50).any(|_| {
            let ok = bindable(("0.0.0.0", FRYNODE_API_PORT))
                && bindable(("127.0.0.1", FRYNODE_API_PORT));
            if !ok {
                std::thread::sleep(Duration::from_millis(100));
            }
            ok
        });
        assert!(
            free,
            "harness: port {FRYNODE_API_PORT} is held on this machine; this scenario would reach \
             health_check's untracked-frynode branch, so it refuses to run"
        );
    }
}
