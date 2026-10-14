//! One-time, gated rebuild of the per-device Diiisco node image.
//!
//! The node image bakes the device API key (and now the upstream pin) in at
//! build time. When either changes (a v0.4.35 device upgrading, or a later pin
//! bump) the image is rebuilt exactly once under a NEW tag, the node is switched
//! to it, and only a node that proves the new key (200 with the key, 401
//! without) is promoted to `latest`. Any failure leaves `latest` — the
//! current-good image — untouched and retries with a bounded backoff.
//!
//! Everything that touches Docker goes through `NodeOps`, so the sequence is
//! tested without Docker. Nothing here ever logs a key value.
use anyhow::Result;
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{info, warn};

pub(super) const STAMP_FILE: &str = "node-image.stamp";
pub(super) const BACKOFF_FILE: &str = "node-rebuild.backoff";
pub(super) const LATEST_REF: &str = "diiisco-node:latest";
const BACKOFF_BASE_SECS: u64 = 3600;
const BACKOFF_CAP_SECS: u64 = 86_400;
pub(super) const PROBE_BUDGET_SECS: u64 = 90;
pub(super) const PROBE_STEP_SECS: u64 = 5;

/// Crash-safe in-process serialisation of plan -> build -> switch. Released on
/// drop, panic and process death; no file whose mere existence blocks.
pub(super) static REBUILD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Bumped by `stop()`. The detached start sequence captures it at entry and
/// refuses to bring the node back up once it has changed.
static STOP_GENERATION: AtomicU64 = AtomicU64::new(0);

pub(super) fn bump_stop_generation() {
    STOP_GENERATION.fetch_add(1, Ordering::SeqCst);
}

pub(super) fn stop_generation() -> u64 {
    STOP_GENERATION.load(Ordering::SeqCst)
}

const EMBEDDED_DOCKERFILE: &str = include_str!("diiisco_deploy/diiisco-node/Dockerfile");

/// Seconds since the unix epoch (0 if the clock is before it).
pub(super) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

/// First 12 hex of sha256(s): the only form in which a key is ever logged.
pub(super) fn fp12(s: &str) -> String {
    sha256_hex(s)[..12].to_string()
}

/// Immutable image tag for a (key, pin) pair.
pub(super) fn image_tag(key_fp: &str, pin_sha: &str) -> String {
    format!("fem-{}", &sha256_hex(&format!("{key_fp}{pin_sha}"))[..12])
}

fn is_hex(s: &str, n: usize) -> bool {
    s.len() == n
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The pin from `ARG DIIISCO_NODE_SHA=<40hex>` in a Dockerfile text.
pub(super) fn pin_sha_of(dockerfile: &str) -> Option<String> {
    dockerfile.lines().find_map(|l| {
        let v = l.trim().strip_prefix("ARG DIIISCO_NODE_SHA=")?;
        is_hex(v, 40).then(|| v.to_string())
    })
}

/// The pin embedded in this build's Dockerfile: the single source of truth.
pub(super) fn pin_sha() -> Option<String> {
    pin_sha_of(include_str!("diiisco_deploy/diiisco-node/Dockerfile"))
}

/// What the running `latest` image was built with. Written only after the new
/// node has proven itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Stamp {
    pub tag: String,
    pub key_fp: String,
    pub pin_sha: String,
}

/// One recorded rebuild attempt for `tag`. Recorded BEFORE the build and
/// cleared only on success, so an interrupted build counts as a failed one.
/// Keyed by the target tag: a new key or pin is not held back by an old
/// failure, while retries of the same target honour the delay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Backoff {
    pub attempts: u32,
    pub last_unix: u64,
    pub tag: String,
}

fn field<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|l| l.strip_prefix(name)?.strip_prefix('='))
        .map(str::trim)
}

fn is_tag(s: &str) -> bool {
    s.strip_prefix("fem-").is_some_and(|h| is_hex(h, 12))
}

/// Unparsable or absent = None (never a hard error).
pub(super) fn read_stamp(dir: &Path) -> Option<Stamp> {
    let t = std::fs::read_to_string(dir.join(STAMP_FILE)).ok()?;
    let tag = field(&t, "tag").filter(|v| is_tag(v))?;
    let key_fp = field(&t, "key_fp").filter(|v| is_hex(v, 12))?;
    let pin = field(&t, "pin_sha").filter(|v| is_hex(v, 40))?;
    Some(Stamp {
        tag: tag.into(),
        key_fp: key_fp.into(),
        pin_sha: pin.into(),
    })
}

pub(super) fn read_backoff(dir: &Path) -> Option<Backoff> {
    let t = std::fs::read_to_string(dir.join(BACKOFF_FILE)).ok()?;
    let attempts: u32 = field(&t, "attempts")?.parse().ok()?;
    let last_unix: u64 = field(&t, "last")?.parse().ok()?;
    let tag = field(&t, "tag").filter(|v| is_tag(v))?;
    (attempts >= 1).then(|| Backoff {
        attempts,
        last_unix,
        tag: tag.into(),
    })
}

/// Unique temp names: two writers of the same target (start() and a detached
/// run_node can both write the Dockerfile) must never share a temp file.
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Atomic write: unique tmp file in the same dir, fsync, rename over the
/// target. On any error the call removes its own temp file.
fn write_atomic(dir: &Path, name: &str, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let seq = TMP_SEQ.fetch_add(1, Ordering::SeqCst);
    let tmp = dir.join(format!(".{name}.{}.{nanos}.{seq}.tmp", std::process::id()));
    let result = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
        drop(f);
        rename_retrying(|| std::fs::rename(&tmp, dir.join(name)))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// The final rename. On Windows it fails with PermissionDenied while another
/// process holds the target open (e.g. `docker compose build` reading the
/// Dockerfile), so that kind alone is retried: 5 attempts, 100 ms apart, at
/// most 0.4 s of blocking. Callers are sync fns reached from async code, so
/// the total stays under 0.5 s.
pub(super) fn rename_retrying<F: FnMut() -> std::io::Result<()>>(
    mut rename: F,
) -> std::io::Result<()> {
    let mut attempt = 1;
    loop {
        match rename() {
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && attempt < 5 => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            other => return other,
        }
    }
}

/// Atomically write the embedded Dockerfile to `<deploy>/diiisco-node/Dockerfile`,
/// replacing any stale copy, so the image is always built from the pinned
/// source this binary carries.
pub(super) fn write_embedded_dockerfile(deploy: &Path) -> std::io::Result<()> {
    write_atomic(
        &deploy.join("diiisco-node"),
        "Dockerfile",
        EMBEDDED_DOCKERFILE,
    )
}

pub(super) fn write_stamp(dir: &Path, s: &Stamp) -> std::io::Result<()> {
    write_atomic(
        dir,
        STAMP_FILE,
        &format!(
            "tag={}\nkey_fp={}\npin_sha={}\n",
            s.tag, s.key_fp, s.pin_sha
        ),
    )
}

/// Record an attempt for `tag` at `now`; the count continues only for the same
/// target tag.
pub(super) fn record_attempt(dir: &Path, tag: &str, now: u64) -> std::io::Result<()> {
    let prev = read_backoff(dir)
        .filter(|b| b.tag == tag)
        .map_or(0, |b| b.attempts);
    write_atomic(
        dir,
        BACKOFF_FILE,
        &format!(
            "attempts={}\nlast={now}\ntag={tag}\n",
            prev.saturating_add(1)
        ),
    )
}

pub(super) fn clear_backoff(dir: &Path) {
    if let Err(e) = std::fs::remove_file(dir.join(BACKOFF_FILE)) {
        if e.kind() != std::io::ErrorKind::NotFound {
            warn!(error = %e, "Diiisco: could not clear rebuild backoff");
        }
    }
}

/// 1 h after the first failed attempt, doubling per attempt, capped at 24 h.
pub(super) fn delay_secs(attempts: u32) -> u64 {
    let shift = attempts.saturating_sub(1).min(10);
    (BACKOFF_BASE_SECS << shift).min(BACKOFF_CAP_SECS)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Plan {
    RunExisting,
    BuildNew { tag: String },
    RunOldDeferred,
}

/// PURE. Decide what start() does. A stamp for this exact key + pin = run what
/// is there. Otherwise a recorded attempt for this target still inside its
/// backoff window defers (run the old image); else build. A backoff stamped in
/// the future (clock moved back) is ignored.
pub(super) fn plan_start(
    stamp: Option<&Stamp>,
    backoff: Option<&Backoff>,
    key_fp: &str,
    pin_sha: &str,
    now: u64,
) -> Plan {
    let tag = image_tag(key_fp, pin_sha);
    if stamp.is_some_and(|s| s.tag == tag && s.key_fp == key_fp && s.pin_sha == pin_sha) {
        return Plan::RunExisting;
    }
    let deferred = backoff
        .filter(|b| b.tag == tag)
        .and_then(|b| {
            now.checked_sub(b.last_unix)
                .map(|el| el < delay_secs(b.attempts))
        })
        .unwrap_or(false);
    if deferred {
        Plan::RunOldDeferred
    } else {
        Plan::BuildNew { tag }
    }
}

/// PURE. Promotion needs the node to accept the device key AND reject a
/// request with no credentials; 200 without a key means the auth override was
/// not loaded.
pub(super) fn probe_verdict(with_key_status: u16, no_key_status: u16) -> bool {
    with_key_status == 200 && no_key_status == 401
}

fn marker(secret: &str) -> String {
    format!("[diiisco-key fp={}]", fp12(secret))
}

/// Mask every provided secret and any full `sk-fem-<64 hex>` key in docker
/// output before it is logged or put in an error.
pub(super) fn redact_node_output(s: &str, secrets: &[&str]) -> String {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new("sk-fem-[0-9a-f]{64}").expect("static regex"));
    let mut sorted: Vec<&str> = secrets.iter().copied().filter(|x| !x.is_empty()).collect();
    sorted.sort_by_key(|x| std::cmp::Reverse(x.len()));
    let mut out = s.to_string();
    for x in sorted {
        out = out.replace(x, &marker(x));
    }
    re.replace_all(&out, |c: &regex::Captures| marker(&c[0]))
        .into_owned()
}

/// Everything the sequence needs from Docker. Output handed back in errors is
/// already redacted.
#[async_trait]
pub(super) trait NodeOps: Send + Sync {
    fn image_exists(&self) -> bool;
    /// Put this build's embedded Dockerfile on disk. Called before every
    /// build: only install() used to write it, so an upgraded device would
    /// otherwise build from a stale (unpinned) copy.
    fn refresh_dockerfile(&self) -> Result<()>;
    fn image_id(&self, reference: &str) -> Option<String>;
    /// `compose build` with `DIIISCO_IMAGE_TAG=<tag>`.
    fn build(&self, tag: &str) -> Result<()>;
    fn down(&self);
    /// The existing `compose up -d` (image resolved from `latest`).
    fn up_plain(&self) -> Result<()>;
    /// `compose up -d --no-build`; `Some(tag)` sets the tag, `None` removes it.
    fn up_no_build(&self, tag: Option<&str>) -> Result<()>;
    /// The AM3 auth probe, with bounded retries while the node boots.
    async fn probe(&self) -> bool;
    fn tag_latest(&self, tag: &str) -> Result<()>;
    fn rmi_by_id(&self, id: &str);
    /// True once `stop()` ran after this sequence began.
    fn stop_requested(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    /// Existing image run (RunExisting / RunOldDeferred).
    Ran,
    /// No image existed: built, promoted, run.
    BuiltFresh,
    /// Upgrade promoted: the new image is `latest` and running.
    Upgraded,
    /// Upgrade failed or was interrupted; the old image runs, backoff kept.
    UpgradeFailed,
    /// `stop()` ran meanwhile: nothing was brought up or promoted.
    Stopped,
}

/// A fresh install built `tag`: make it `latest` BEFORE stamping, so a
/// recovery `up` that carries no tag env never finds `latest` missing.
pub(super) fn after_fresh_build<O: NodeOps + ?Sized>(
    ops: &O,
    dir: &Path,
    tag: &str,
    key_fp: &str,
    pin_sha: &str,
) -> Result<()> {
    ops.tag_latest(tag)?;
    write_stamp(
        dir,
        &Stamp {
            tag: tag.to_string(),
            key_fp: key_fp.to_string(),
            pin_sha: pin_sha.to_string(),
        },
    )?;
    clear_backoff(dir);
    Ok(())
}

/// Old image back up after a failed or interrupted upgrade. `latest` was never
/// moved, so compose resolves it without any tag env.
fn fallback_old<O: NodeOps + ?Sized>(ops: &O) -> Result<Outcome> {
    if ops.stop_requested() {
        info!("Diiisco: stopped during rebuild — leaving the node down");
        return Ok(Outcome::Stopped);
    }
    ops.up_no_build(None)?;
    Ok(Outcome::UpgradeFailed)
}

/// plan -> build -> switch, all under `REBUILD_LOCK`. A second concurrent
/// caller waits here, re-plans, sees the stamp and runs the image: one build.
///
/// `clock` is read only AFTER the lock is held: a waiter that queued behind a
/// long build must plan against the time it actually gets the lock, not the
/// time it queued, or it would see an attempt "from the future" and rebuild.
/// If a start() is dropped (the 60 s toggle bound) while this runs detached,
/// later queued starts simply find the stamp and run the image; a stop() can
/// still land between the last `stop_requested` check and the `up` it guards
/// (the check-to-up window), which the next health cycle absorbs.
pub(super) async fn run_node_sequence<O: NodeOps + ?Sized, C: Fn() -> u64>(
    ops: &O,
    dir: &Path,
    key_fp: &str,
    pin_sha: &str,
    clock: C,
) -> Result<Outcome> {
    let _guard = REBUILD_LOCK.lock().await;
    let now = clock();
    let have_image = ops.image_exists();
    let plan = plan_start(
        read_stamp(dir).as_ref(),
        read_backoff(dir).as_ref(),
        key_fp,
        pin_sha,
        now,
    );
    let tag = image_tag(key_fp, pin_sha);
    // With no image there is nothing to protect, so a backoff never blocks it.
    let must_build = !have_image || matches!(plan, Plan::BuildNew { .. });
    let upgrade = have_image && must_build;

    if !must_build {
        ops.down();
        if ops.stop_requested() {
            return Ok(Outcome::Stopped);
        }
        ops.up_plain()?;
        return Ok(Outcome::Ran);
    }

    if !upgrade {
        info!("Diiisco: no node image — building before start");
        ops.refresh_dockerfile()?;
        ops.build(&tag)?;
        if ops.stop_requested() {
            return Ok(Outcome::Stopped);
        }
        after_fresh_build(ops, dir, &tag, key_fp, pin_sha)?;
        ops.down();
        if ops.stop_requested() {
            return Ok(Outcome::Stopped);
        }
        ops.up_plain()?;
        return Ok(Outcome::BuiltFresh);
    }

    // Upgrade: the old container keeps running while the new image builds.
    let prev_id = ops.image_id(LATEST_REF);
    if let Err(e) = record_attempt(dir, &tag, now) {
        warn!(error = %e, "Diiisco: cannot record rebuild attempt — keeping the current node");
        return fallback_old(ops);
    }
    if let Err(e) = ops.refresh_dockerfile() {
        warn!(error = %e, "Diiisco: cannot refresh the node Dockerfile — keeping the current node");
        return fallback_old(ops);
    }
    info!(fp = %key_fp, "Diiisco: rebuilding node image for the per-device key");
    if let Err(e) = ops.build(&tag) {
        warn!(error = %e, "Diiisco: upgrade build failed — keeping the current node");
        return fallback_old(ops);
    }
    if ops.stop_requested() {
        info!("Diiisco: stopped during rebuild — not switching");
        return Ok(Outcome::Stopped);
    }
    ops.down();
    if ops.stop_requested() {
        return Ok(Outcome::Stopped);
    }
    if let Err(e) = ops.up_no_build(Some(&tag)) {
        warn!(error = %e, "Diiisco: new node failed to start — restoring the current node");
        return fallback_old(ops);
    }
    if !ops.probe().await {
        warn!("Diiisco: new node did not prove the device key — restoring the current node");
        return fallback_old(ops);
    }
    if ops.stop_requested() {
        return Ok(Outcome::Stopped);
    }
    if let Err(e) = ops.tag_latest(&tag) {
        warn!(error = %e, "Diiisco: could not promote the new image — restoring the current node");
        return fallback_old(ops);
    }
    let stamp = Stamp {
        tag: tag.clone(),
        key_fp: key_fp.to_string(),
        pin_sha: pin_sha.to_string(),
    };
    if let Err(e) = write_stamp(dir, &stamp) {
        warn!(error = %e, "Diiisco: could not write the image stamp (will retry after backoff)");
        return Ok(Outcome::Upgraded);
    }
    clear_backoff(dir);
    let new_id = ops.image_id(&format!("diiisco-node:{tag}"));
    if let Some(prev) = prev_id.filter(|p| Some(p) != new_id.as_ref()) {
        ops.rmi_by_id(&prev);
    }
    Ok(Outcome::Upgraded)
}
