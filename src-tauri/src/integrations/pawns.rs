//! Pawns.app (IPRoyal) residential bandwidth sharing.
//!
//! Delivery: the official `iproyal/pawns-cli` container image. IPRoyal publishes the
//! CLI agent as a Linux binary (`download.iproyal.com/pawns-cli/latest/linux_*`) and as
//! this image; there is no Windows build of the CLI, so FEM runs the official image
//! through the Docker engine it already manages for its other container integrations.
//!
//! Credentials come from the process environment (`PAWNS_EMAIL` / `PAWNS_PASSWORD`),
//! matching how the other partner integrations take their secrets, and are passed
//! straight to `docker run` — nothing is written to disk.
//!
//! Consent: the Pawns.app CLI Addendum (§5.2–5.4) requires a separate, explicit consent
//! action from the person who owns the device *before* the agent starts, disclosing what
//! sharing does, and (§5.8) a durable record of each consent and withdrawal. `start()`
//! refuses to run without `PAWNS_USER_CONSENT=accepted` and appends a record of the exact
//! wording shown to `consent-log.jsonl`; `stop()` records the withdrawal.

use super::download::partners_base_dir;
use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::supervisor::platform::{BoundedOutput, LONG_TIMEOUT, PROBE_TIMEOUT};

/// Official Pawns.app CLI agent image.
const PAWNS_IMAGE: &str = "iproyal/pawns-cli:latest";
/// Container FEM manages. Fixed name so a restart adopts the same container.
const PAWNS_CONTAINER: &str = "fem-pawns";
/// Version of the disclosure wording below; recorded with every consent entry.
const CONSENT_WORDING_VERSION: &str = "1";

/// Exact wording the device owner must be shown before sharing starts
/// (CLI Addendum §5.4 (a)–(e)). Surfaced through the integration's health
/// message until consent is given, and stored with each consent record.
const CONSENT_DISCLOSURE: &str = "Pawns.app bandwidth sharing: internet traffic from Pawns.app and \
its customers is routed through this device and its internet connection; the device's public IP \
address and technical connection data are visible to those customers; sharing uses processor \
capacity, battery and data allowance and can slow the connection. Only enable it if you are the \
main user of this device and connection, you have reached the age of majority where you live, and \
your internet provider's terms allow commercial traffic sharing. Turn the Pawns.app integration \
off at any time to stop sharing immediately.";

/// What the agent's event stream says it is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PawnsState {
    /// Authenticated and sharing.
    Sharing,
    /// Authenticated but the agent reports it is not sharing, with the reason.
    Blocked(String),
    /// Started, nothing conclusive yet.
    Starting,
    /// No recognised event in the log.
    Unknown,
}

pub struct PawnsIntegration;

impl PawnsIntegration {
    fn partner_dir() -> PathBuf {
        partners_base_dir().join("pawns")
    }

    /// Written by `install()`; lets `installed_version()` answer without
    /// spawning a process (it runs inside the registry lock).
    fn install_marker() -> PathBuf {
        Self::partner_dir().join("installed.json")
    }

    fn consent_log() -> PathBuf {
        Self::partner_dir().join("consent-log.jsonl")
    }

    /// Whether sharing is allowed to start: the headless override, or a consent
    /// this device recorded and has not withdrawn.
    pub(crate) fn user_consent() -> bool {
        consent_from_env_value(std::env::var("PAWNS_USER_CONSENT").ok().as_deref())
            || Self::consent_active()
    }

    /// The consent/withdrawal this device last recorded, or None if it never has.
    pub(crate) fn consent_record() -> Option<ConsentRecord> {
        last_consent_entry_in(&Self::consent_log(), &Self::device_id())
    }

    /// Whether this device currently holds a recorded consent.
    pub(crate) fn consent_active() -> bool {
        consent_is_active(&Self::consent_log(), &Self::device_id())
    }

    /// Record the device owner consenting (CLI Addendum §5.8).
    pub(crate) fn grant_consent() {
        Self::record_consent_event("consent");
    }

    /// Record the device owner withdrawing consent (CLI Addendum §5.8).
    pub(crate) fn revoke_consent() {
        Self::record_consent_event("withdrawal");
    }

    fn credentials() -> Option<(String, String)> {
        let email = std::env::var("PAWNS_EMAIL").ok().filter(|s| !s.is_empty())?;
        let password = std::env::var("PAWNS_PASSWORD")
            .ok()
            .filter(|s| !s.is_empty())?;
        Some((email, password))
    }

    fn host_label() -> String {
        std::env::var("COMPUTERNAME")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "fem-device".to_string())
    }

    fn device_name() -> String {
        std::env::var("PAWNS_DEVICE_NAME")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(Self::host_label)
    }

    fn device_id() -> String {
        std::env::var("PAWNS_DEVICE_ID")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("fem-{}", sanitize_id(&Self::host_label())))
    }

    /// Append a consent or withdrawal record (CLI Addendum §5.8).
    fn record_consent_event(action: &str) {
        Self::record_consent_event_at(&Self::consent_log(), action);
    }

    /// The write itself, against an explicit path so tests can exercise the
    /// record format without touching the real device's consent log.
    fn record_consent_event_at(path: &Path, action: &str) {
        let record = serde_json::json!({
            "action": action,
            "happened_at": utc_now_rfc3339(),
            "device_id": Self::device_id(),
            "device_name": Self::device_name(),
            "wording_version": CONSENT_WORDING_VERSION,
            "wording_language": "en",
            "wording": CONSENT_DISCLOSURE,
            "agent_image": PAWNS_IMAGE,
            "fem_version": env!("CARGO_PKG_VERSION"),
        });
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                warn!(error = %e, "Could not create Pawns partner directory for consent log");
                return;
            }
        }
        let line = format!("{}\n", record);
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            Ok(mut f) => {
                use std::io::Write;
                if let Err(e) = f.write_all(line.as_bytes()) {
                    warn!(error = %e, "Could not append Pawns consent record");
                }
            }
            Err(e) => warn!(error = %e, "Could not open Pawns consent log"),
        }
    }

    fn docker_available() -> bool {
        super::docker_manager::docker_cli_probe_bounded() == Some(true)
    }

    /// `running` / `exited` / … for the managed container, or None when absent.
    fn container_state() -> Option<String> {
        let output = crate::supervisor::platform::command("docker")
            .args(["inspect", "-f", "{{.State.Status}}", PAWNS_CONTAINER])
            .output_bounded(PROBE_TIMEOUT)
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let state = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if state.is_empty() {
            None
        } else {
            Some(state)
        }
    }

    fn container_logs(tail: &str) -> Option<String> {
        let output = crate::supervisor::platform::command("docker")
            .args(["logs", "--tail", tail, PAWNS_CONTAINER])
            .output_bounded(PROBE_TIMEOUT)
            .ok()?;
        Some(format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

/// The consent or withdrawal a device last recorded.
pub(crate) struct ConsentRecord {
    /// `consent` or `withdrawal` — the audit vocabulary written to the log.
    pub action: String,
    /// RFC3339 UTC timestamp of that decision.
    pub happened_at: String,
}

/// The audited disclosure the device owner has to be shown (§5.4 (a)–(e)).
/// Handed to the UI so the wording exists in exactly one place.
pub(crate) fn consent_disclosure() -> &'static str {
    CONSENT_DISCLOSURE
}

/// Version of the wording above, recorded with every consent entry.
pub(crate) fn consent_wording_version() -> &'static str {
    CONSENT_WORDING_VERSION
}

/// The documented headless escape hatch: `PAWNS_USER_CONSENT=accepted`.
fn consent_from_env_value(value: Option<&str>) -> bool {
    value
        .map(|v| v.trim().eq_ignore_ascii_case("accepted"))
        .unwrap_or(false)
}

/// The last consent entry `device_id` wrote to the log at `path`.
///
/// The log is append-only and one device per line, so the newest entry for this
/// device is the decision that stands. Lines that are not usable records are
/// skipped rather than ending the scan: a half-written trailing line (power
/// loss mid-append) must not silently withdraw a consent the owner gave.
fn last_consent_entry_in(path: &Path, device_id: &str) -> Option<ConsentRecord> {
    let body = std::fs::read_to_string(path).ok()?;
    let mut found = None;
    for line in body.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("device_id").and_then(|v| v.as_str()) != Some(device_id) {
            continue;
        }
        let Some(action) = value.get("action").and_then(|v| v.as_str()) else {
            continue;
        };
        found = Some(ConsentRecord {
            action: action.to_string(),
            happened_at: value
                .get("happened_at")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        });
    }
    found
}

/// Whether `device_id`'s last recorded decision was a consent. No log, no
/// readable entry, or a withdrawal all mean no — the gate fails closed.
fn consent_is_active(path: &Path, device_id: &str) -> bool {
    last_consent_entry_in(path, device_id)
        .map(|e| e.action == "consent")
        .unwrap_or(false)
}

/// Keep a device id to characters the API accepts.
fn sanitize_id(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

/// RFC3339 UTC timestamp without pulling in a date crate.
fn utc_now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m,
        d,
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

/// Howard Hinnant's days-from-civil, inverted (proleptic Gregorian).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Classify the agent's NDJSON event stream. The last recognised event wins:
/// `balance_ready` proves the agent authenticated and is live, `not_running`
/// means it stopped sharing and carries the reason.
pub fn classify_events(logs: &str) -> PawnsState {
    let mut state = PawnsState::Unknown;
    for line in logs.lines() {
        if !line.contains("\"name\"") {
            continue;
        }
        if line.contains("\"name\":\"not_running\"") {
            state = PawnsState::Blocked(explain_block(line));
        } else if line.contains("\"name\":\"balance_ready\"")
            || line.contains("\"name\":\"running\"")
        {
            state = PawnsState::Sharing;
        } else if line.contains("\"name\":\"starting\"") && state == PawnsState::Unknown {
            state = PawnsState::Starting;
        }
    }
    state
}

/// Turn a `not_running` event into something a device owner can act on.
fn explain_block(line: &str) -> String {
    let code = json_string_field(line, "error").unwrap_or_default();
    match code.as_str() {
        "ip_used" => "Another Pawns.app peer is already sharing this network's public IP address. \
Pawns.app accepts one peer per IP, so stop the other Pawns.app instance on this network to share \
from this device."
            .to_string(),
        "" => json_string_field(line, "message")
            .unwrap_or_else(|| "Pawns.app agent reported it is not sharing".to_string()),
        other => {
            let detail = json_string_field(line, "message").unwrap_or_default();
            if detail.is_empty() {
                format!("Pawns.app agent is not sharing ({})", other)
            } else {
                format!("Pawns.app agent is not sharing ({}): {}", other, detail)
            }
        }
    }
}

/// Whether the device is actually sharing right now, from the container's state
/// and its own event stream. Proof of activity, not proof of installation: a
/// running container that the agent reports as `not_running` earns nothing.
pub fn poa_from(container_state: Option<&str>, logs: &str) -> bool {
    container_state == Some("running") && classify_events(logs) == PawnsState::Sharing
}

/// Minimal string-field reader for the agent's flat event JSON.
fn json_string_field(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", key);
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

#[async_trait]
impl Integration for PawnsIntegration {
    fn id(&self) -> &str {
        "pawns"
    }

    fn display_name(&self) -> &str {
        "Pawns.app"
    }

    async fn install(&self) -> Result<()> {
        super::docker_manager::ensure_docker().await?;
        tokio::fs::create_dir_all(Self::partner_dir()).await?;

        info!(image = PAWNS_IMAGE, "Pulling Pawns.app CLI agent image");
        let output = crate::supervisor::platform::command("docker")
            .args(["pull", PAWNS_IMAGE])
            .output_bounded(LONG_TIMEOUT)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(stderr = %stderr, "docker pull failed for Pawns.app agent");
            anyhow::bail!(
                "Could not download the Pawns.app agent image: {}",
                super::stderr_tail(&stderr, 10)
            );
        }

        let marker = serde_json::json!({
            "image": PAWNS_IMAGE,
            "pulled_at": utc_now_rfc3339(),
        });
        tokio::fs::write(Self::install_marker(), serde_json::to_vec_pretty(&marker)?).await?;
        info!("Pawns.app agent image installed");
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        if !Self::user_consent() {
            anyhow::bail!(
                "{} Enable this integration to consent (sets PAWNS_USER_CONSENT=accepted).",
                CONSENT_DISCLOSURE
            );
        }

        let (email, password) = match Self::credentials() {
            Some(creds) => creds,
            None => anyhow::bail!(
                "Pawns.app sign-in details are missing. Set PAWNS_EMAIL and PAWNS_PASSWORD for the \
                 Pawns.app account this device shares under, then enable the integration again."
            ),
        };

        super::docker_manager::ensure_docker().await?;
        if !Self::install_marker().exists() {
            self.install().await?;
        }

        Self::record_consent_event("consent");

        // Drop any container left from a previous run so the fixed name is free
        // and the agent restarts with current credentials.
        let _ = crate::supervisor::platform::command("docker")
            .args(["rm", "-f", PAWNS_CONTAINER])
            .output_bounded(PROBE_TIMEOUT);

        let device_name = Self::device_name();
        let device_id = Self::device_id();
        info!(
            device_name = %device_name,
            device_id = %device_id,
            "Starting Pawns.app agent"
        );

        // Credentials are passed as arguments because the agent takes no other
        // input; they are never logged and never written to disk.
        let output = crate::supervisor::platform::command("docker")
            .args([
                "run",
                "-d",
                "--name",
                PAWNS_CONTAINER,
                "--restart",
                "unless-stopped",
                PAWNS_IMAGE,
                &format!("-email={}", email),
                &format!("-password={}", password),
                &format!("-device-name={}", device_name),
                &format!("-device-id={}", device_id),
                "-accept-tos",
            ])
            .output_bounded(LONG_TIMEOUT)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(stderr = %stderr, "Failed to start Pawns.app agent");
            anyhow::bail!(
                "Could not start the Pawns.app agent: {}",
                super::stderr_tail(&stderr, 10)
            );
        }

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        let output = crate::supervisor::platform::command("docker")
            .args(["rm", "-f", PAWNS_CONTAINER])
            .output_bounded(PROBE_TIMEOUT);
        match output {
            Ok(o) if o.status.success() => info!("Pawns.app agent stopped — sharing ended"),
            Ok(_) => info!("Pawns.app agent was not running"),
            Err(e) => warn!(error = %e, "Could not stop the Pawns.app agent"),
        }
        Self::record_consent_event("withdrawal");
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        if !Self::user_consent() {
            return HealthStatus::Unhealthy(format!(
                "{} Enable this integration to consent.",
                CONSENT_DISCLOSURE
            ));
        }
        if Self::credentials().is_none() {
            return HealthStatus::Unhealthy(
                "Pawns.app sign-in details are missing — set PAWNS_EMAIL and PAWNS_PASSWORD."
                    .to_string(),
            );
        }
        if !Self::docker_available() {
            return HealthStatus::Unhealthy(super::docker_manager::status_user_message(
                super::docker_manager::docker_status(),
            ));
        }

        let state = match Self::container_state() {
            Some(s) => s,
            None => return HealthStatus::Stopped,
        };
        if state != "running" {
            let detail = Self::container_logs("20").unwrap_or_default();
            return match classify_events(&detail) {
                PawnsState::Blocked(reason) => HealthStatus::Unhealthy(reason),
                _ => HealthStatus::Unhealthy(format!("Pawns.app agent is {}", state)),
            };
        }

        // Running is not the same as sharing — the agent reports that itself.
        let logs = match Self::container_logs("50") {
            Some(l) => l,
            // Confirmed running but the log is unreadable: unverified, not healthy.
            None => return HealthStatus::Starting,
        };
        match classify_events(&logs) {
            PawnsState::Sharing => HealthStatus::Healthy,
            PawnsState::Blocked(reason) => HealthStatus::Unhealthy(reason),
            PawnsState::Starting | PawnsState::Unknown => HealthStatus::Starting,
        }
    }

    async fn check_update(&self) -> Result<Option<String>> {
        Ok(None) // the image is pulled by tag; `apply_update` re-pulls it
    }

    async fn apply_update(&self, _version: &str) -> Result<()> {
        self.install().await
    }

    fn installed_version(&self) -> Option<String> {
        if Self::install_marker().exists() {
            Some("latest".to_string())
        } else {
            None
        }
    }

    fn collect_poc_data(&self) -> PocGateData {
        // Runs synchronously inside the PoC tick, outside the health map's
        // timeout, so it must stay cheap: a filesystem check short-circuits
        // before any process spawn, and both probes below are bounded.
        let poa = if Self::install_marker().exists() {
            poa_from(
                Self::container_state().as_deref(),
                &Self::container_logs("50").unwrap_or_default(),
            )
        } else {
            false
        };
        PocGateData {
            poa,
            ..Default::default()
        }
    }

    fn requires_docker(&self) -> bool {
        true
    }
}

/// Consent-log behaviour has its own file so these tests can reach the
/// module-private log helpers without widening them for the whole crate.
#[cfg(test)]
#[path = "pawns_consent_tests.rs"]
mod pawns_consent_tests;

#[cfg(test)]
mod tests {
    use super::*;

    const SHARING: &str = r#"{"happened_at":"2026-08-11T21:48:52Z","name":"balance_ready","parameters":{"balance":"0.000 USD","traffic":"0.0000 GB"}}"#;
    const BLOCKED: &str = r#"{"happened_at":"2026-08-11T21:49:03Z","name":"not_running","parameters":{"error":"ip_used","message":"getting free port failed (ip has alive peer)"}}"#;
    const STARTING: &str = r#"{"happened_at":"2026-08-11T21:48:50Z","name":"starting","parameters":{}}"#;
    const RUNNING: &str = r#"{"happened_at":"2026-08-12T17:59:40Z","name":"running","parameters":{}}"#;

    #[test]
    fn reports_sharing_when_the_agent_last_reported_a_balance() {
        let logs = format!("{}\n{}\n", STARTING, SHARING);
        assert_eq!(classify_events(&logs), PawnsState::Sharing);
    }

    #[test]
    fn a_later_not_running_event_overrides_an_earlier_balance() {
        let logs = format!("{}\n{}\n{}\n", STARTING, SHARING, BLOCKED);
        match classify_events(&logs) {
            PawnsState::Blocked(reason) => {
                assert!(reason.contains("one peer per IP"), "reason was: {}", reason)
            }
            other => panic!("expected Blocked, got {:?}", other),
        }
    }

    #[test]
    fn a_recovery_after_a_block_reports_sharing_again() {
        let logs = format!("{}\n{}\n{}\n", STARTING, BLOCKED, SHARING);
        assert_eq!(classify_events(&logs), PawnsState::Sharing);
    }

    #[test]
    fn only_a_start_event_is_not_yet_sharing() {
        assert_eq!(classify_events(STARTING), PawnsState::Starting);
    }

    #[test]
    fn an_empty_or_unrecognised_log_is_unknown() {
        assert_eq!(classify_events(""), PawnsState::Unknown);
        assert_eq!(
            classify_events("pulling image\nno events here\n"),
            PawnsState::Unknown
        );
    }

    #[test]
    fn an_unknown_error_code_keeps_the_agents_own_message() {
        let line = r#"{"name":"not_running","parameters":{"error":"auth_failed","message":"invalid credentials"}}"#;
        match classify_events(line) {
            PawnsState::Blocked(reason) => {
                assert!(reason.contains("auth_failed"), "reason was: {}", reason);
                assert!(
                    reason.contains("invalid credentials"),
                    "reason was: {}",
                    reason
                );
            }
            other => panic!("expected Blocked, got {:?}", other),
        }
    }

    #[test]
    fn the_consent_disclosure_covers_every_point_the_addendum_requires() {
        let text = CONSENT_DISCLOSURE.to_lowercase();
        assert!(text.contains("routed through this device")); // (a) traffic routing
        assert!(text.contains("public ip address")); // (b) IP visibility
        assert!(text.contains("data allowance")); // (c) resource use
        assert!(text.contains("age of majority")); // (d) eligibility
        assert!(text.contains("off at any time")); // (e) how to stop
    }

    #[test]
    fn device_ids_stay_within_the_accepted_character_set() {
        assert_eq!(sanitize_id("FryStation"), "frystation");
        assert_eq!(sanitize_id("Sam's PC (2)"), "sam-s-pc--2-");
    }

    #[test]
    fn timestamps_are_rfc3339_utc() {
        let ts = utc_now_rfc3339();
        assert!(ts.ends_with('Z'), "{}", ts);
        assert_eq!(ts.len(), 20, "{}", ts);
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[10..11], "T");
    }

    #[test]
    fn poa_is_true_while_the_agent_reports_sharing() {
        let logs = format!("{}\n{}\n", STARTING, SHARING);
        assert!(poa_from(Some("running"), &logs));
    }

    #[test]
    fn poa_is_true_on_an_explicit_running_event() {
        let logs = format!("{}\n{}\n", STARTING, RUNNING);
        assert!(poa_from(Some("running"), &logs));
    }

    #[test]
    fn poa_is_false_while_the_ip_conflict_blocks_sharing() {
        let logs = format!("{}\n{}\n{}\n", STARTING, SHARING, BLOCKED);
        assert!(!poa_from(Some("running"), &logs));
    }

    #[test]
    fn poa_is_false_when_the_container_is_not_running() {
        let logs = format!("{}\n{}\n", STARTING, SHARING);
        assert!(!poa_from(Some("exited"), &logs));
        assert!(!poa_from(None, &logs));
    }

    #[test]
    fn poa_is_false_before_the_agent_has_reported_anything() {
        assert!(!poa_from(Some("running"), STARTING));
        assert!(!poa_from(Some("running"), ""));
    }

    #[test]
    fn civil_dates_match_known_days_since_epoch() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1)); // leap-year boundary
        assert_eq!(civil_from_days(20_676), (2026, 8, 11));
    }

    /// Samples the real agent through the same path the PoC tick uses, and
    /// requires it to report activity while it is sharing. Opt-in for the same
    /// reasons as the lifecycle test below.
    #[tokio::test]
    #[ignore = "needs Docker, network and real Pawns.app credentials"]
    async fn pawns_live_poa_tracks_the_agent() {
        let pawns = PawnsIntegration;
        assert!(
            PawnsIntegration::install_marker().exists(),
            "agent not installed — run the lifecycle test first"
        );

        let mut samples = Vec::new();
        for _ in 0..15 {
            let state = PawnsIntegration::container_state();
            let logs = PawnsIntegration::container_logs("50").unwrap_or_default();
            let expected = poa_from(state.as_deref(), &logs);
            let reported = pawns.collect_poc_data().poa;
            assert_eq!(
                reported, expected,
                "collect_poc_data disagreed with the agent's own state"
            );
            samples.push((state.unwrap_or_else(|| "absent".into()), reported));
            tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        }

        println!("poa samples: {:?}", samples);
        assert!(
            samples.iter().any(|(_, poa)| *poa),
            "agent never reported activity across {} samples: {:?}",
            samples.len(),
            samples
        );
    }

    /// Drives the real lifecycle against the real Docker engine and the real
    /// Pawns.app account. Opt-in (`cargo test -- --ignored pawns_live`) because
    /// it needs Docker, network, and `PAWNS_EMAIL`/`PAWNS_PASSWORD` in the
    /// environment — CI has none of those.
    #[tokio::test]
    #[ignore = "needs Docker, network and real Pawns.app credentials"]
    async fn pawns_live_lifecycle() {
        let pawns = PawnsIntegration;

        pawns.install().await.expect("install should pull the agent image");
        assert_eq!(pawns.installed_version().as_deref(), Some("latest"));

        pawns.start().await.expect("start should launch the agent");
        assert_eq!(
            PawnsIntegration::container_state().as_deref(),
            Some("running"),
            "agent container should be running after start"
        );

        // The agent authenticates a few seconds after launch; poll until it
        // reports something conclusive rather than asserting on a race.
        let mut observed = Vec::new();
        for _ in 0..12 {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let health = pawns.health_check().await;
            observed.push(health.clone());
            if !matches!(health, HealthStatus::Starting) {
                break;
            }
        }
        let last = observed.last().expect("at least one health check");
        assert!(
            !matches!(last, HealthStatus::Stopped),
            "agent stopped unexpectedly; observed: {:?}",
            observed
        );

        let logs = PawnsIntegration::container_logs("100").unwrap_or_default();
        assert!(
            logs.contains("balance_ready"),
            "agent never authenticated; logs: {}",
            logs
        );

        let consent_log = std::fs::read_to_string(PawnsIntegration::consent_log())
            .expect("start should have written a consent record");
        assert!(consent_log.contains("\"action\":\"consent\""));

        pawns.stop().await.expect("stop should remove the agent");
        assert_eq!(
            PawnsIntegration::container_state(),
            None,
            "container should be gone after stop"
        );
        let consent_log = std::fs::read_to_string(PawnsIntegration::consent_log()).unwrap();
        assert!(consent_log.contains("\"action\":\"withdrawal\""));
    }
}

