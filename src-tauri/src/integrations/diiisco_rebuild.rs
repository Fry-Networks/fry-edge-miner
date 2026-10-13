//! RED stub: real types and signatures, deliberately inert behaviour.
#![allow(dead_code)]
use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;

pub(super) const STAMP_FILE: &str = "node-image.stamp";
pub(super) const BACKOFF_FILE: &str = "node-rebuild.backoff";
pub(super) const LATEST_REF: &str = "diiisco-node:latest";
pub(super) const PROBE_BUDGET_SECS: u64 = 90;
pub(super) const PROBE_STEP_SECS: u64 = 5;

pub(super) static REBUILD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(super) fn unix_now() -> u64 {
    0
}

pub(super) fn write_embedded_dockerfile(_deploy: &Path) -> std::io::Result<()> {
    Ok(())
}

pub(super) fn bump_stop_generation() {}

pub(super) fn stop_generation() -> u64 {
    0
}

pub(super) fn fp12(_s: &str) -> String {
    String::new()
}

pub(super) fn image_tag(_key_fp: &str, _pin_sha: &str) -> String {
    String::new()
}

pub(super) fn pin_sha_of(_dockerfile: &str) -> Option<String> {
    None
}

pub(super) fn pin_sha() -> Option<String> {
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Stamp {
    pub tag: String,
    pub key_fp: String,
    pub pin_sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Backoff {
    pub attempts: u32,
    pub last_unix: u64,
    pub tag: String,
}

pub(super) fn read_stamp(_dir: &Path) -> Option<Stamp> {
    None
}

pub(super) fn read_backoff(_dir: &Path) -> Option<Backoff> {
    None
}

pub(super) fn write_stamp(_dir: &Path, _s: &Stamp) -> std::io::Result<()> {
    Ok(())
}

pub(super) fn record_attempt(_dir: &Path, _tag: &str, _now: u64) -> std::io::Result<()> {
    Ok(())
}

pub(super) fn clear_backoff(_dir: &Path) {}

pub(super) fn delay_secs(_attempts: u32) -> u64 {
    0
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Plan {
    RunExisting,
    BuildNew { tag: String },
    RunOldDeferred,
}

pub(super) fn plan_start(
    _stamp: Option<&Stamp>,
    _backoff: Option<&Backoff>,
    _key_fp: &str,
    _pin_sha: &str,
    _now: u64,
) -> Plan {
    Plan::BuildNew { tag: String::new() }
}

pub(super) fn probe_verdict(_with_key_status: u16, _no_key_status: u16) -> bool {
    false
}

pub(super) fn redact_node_output(s: &str, _secrets: &[&str]) -> String {
    s.to_string()
}

#[async_trait]
pub(super) trait NodeOps: Send + Sync {
    fn image_exists(&self) -> bool;
    fn refresh_dockerfile(&self) -> Result<()>;
    fn image_id(&self, reference: &str) -> Option<String>;
    fn build(&self, tag: &str) -> Result<()>;
    fn down(&self);
    fn up_plain(&self) -> Result<()>;
    fn up_no_build(&self, tag: Option<&str>) -> Result<()>;
    async fn probe(&self) -> bool;
    fn tag_latest(&self, tag: &str) -> Result<()>;
    fn rmi_by_id(&self, id: &str);
    fn stop_requested(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Ran,
    BuiltFresh,
    Upgraded,
    UpgradeFailed,
    Stopped,
}

pub(super) fn after_fresh_build<O: NodeOps + ?Sized>(
    _ops: &O,
    _dir: &Path,
    _tag: &str,
    _key_fp: &str,
    _pin_sha: &str,
) -> Result<()> {
    Ok(())
}

pub(super) async fn run_node_sequence<O: NodeOps + ?Sized, C: Fn() -> u64>(
    _ops: &O,
    _dir: &Path,
    _key_fp: &str,
    _pin_sha: &str,
    _clock: C,
) -> Result<Outcome> {
    Ok(Outcome::Ran)
}
