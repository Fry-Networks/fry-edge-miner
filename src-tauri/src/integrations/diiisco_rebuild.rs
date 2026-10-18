//! One-time, gated rebuild of the per-device Diiisco node image.
//!
//! The node image bakes the device API key (and now the upstream pin) in at
//! build time. When either changes (a v0.4.35 device upgrading, or a later pin
//! bump) the image is rebuilt exactly once under a NEW tag, the node is switched
//! to it, and only a node that proves the new key (401 without it, and a
//! status other than 401/403 with it) is promoted to `latest`. Any failure leaves `latest` — the
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
/// A3: the fresh (no image) start path's own cooldown, written only when a
/// candidate failed to prove the device key. Kept apart from `BACKOFF_FILE`,
/// which the fresh path deliberately ignores.
pub(super) const FRESH_COOLDOWN_FILE: &str = "node-fresh-reject.cooldown";
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
const EMBEDDED_COMPOSE: &str = include_str!("diiisco_deploy/docker-compose.yml");

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

/// sha256 over the Dockerfile bytes followed by the compose bytes: 64 hex.
pub(super) fn compute_recipe_sha(dockerfile: &str, compose: &str) -> String {
    let mut h = Sha256::new();
    h.update(dockerfile.as_bytes());
    h.update(compose.as_bytes());
    hex::encode(h.finalize())
}

/// The recipe this build deploys: its embedded Dockerfile and compose. A change
/// to either changes every image tag, so a device rebuilds once, gated.
pub(super) fn recipe_sha() -> &'static str {
    static RECIPE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    RECIPE.get_or_init(|| compute_recipe_sha(EMBEDDED_DOCKERFILE, EMBEDDED_COMPOSE))
}

/// Immutable image tag for a (key, pin, recipe) triple.
pub(super) fn image_tag_with(key_fp: &str, pin_sha: &str, recipe: &str) -> String {
    format!(
        "fem-{}",
        &sha256_hex(&format!("{key_fp}{pin_sha}{recipe}"))[..12]
    )
}

/// Immutable image tag for a (key, pin) pair under this build's recipe.
pub(super) fn image_tag(key_fp: &str, pin_sha: &str) -> String {
    image_tag_with(key_fp, pin_sha, recipe_sha())
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

/// Stand-in pin for a Dockerfile that carries none. Lowercase letters only (no
/// sed/JSON hazard) and not 40 hex, so it can never equal a real pin.
pub(super) const UNPINNED: &str = "unpinned";

/// The pin that goes into tags and stamps: the real one, else `UNPINNED`.
pub(super) fn pin_for_tag() -> String {
    pin_sha().unwrap_or_else(|| UNPINNED.to_string())
}

/// Whether an EXISTING node image may be rebuilt on a key/pin change.
/// Enabled again with the pin restored in the Dockerfile and the F2 probe
/// (auth proven by 401 without a key and a non-401/403 with it, since the mesh
/// is not reachable from a sandbox). An enabled rebuild without a pin errors.
pub(super) const REBUILD_ENABLED: bool = true;

/// PURE. With the upgrade rebuild disabled, an existing image is never
/// rebuilt: `BuildNew` becomes `RunExisting`. A missing image is unaffected.
pub(super) fn gate_plan(plan: Plan, rebuild_enabled: bool, have_image: bool) -> Plan {
    match plan {
        Plan::BuildNew { .. } if !rebuild_enabled && have_image => Plan::RunExisting,
        other => other,
    }
}

/// Whether the node image exists. `Unknown` = the Docker probe itself failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ImageState {
    Present,
    Absent,
    Unknown,
}

/// What install() does once it holds `REBUILD_LOCK`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InstallStep {
    Build,
    AlreadyBuilt,
    Unknown,
}

/// PURE. Decided under `REBUILD_LOCK`, so a caller that queued behind a build
/// sees its result: an image that exists with a stamp for this key + pin is
/// already built; an unprobeable image is never built over.
pub(super) fn install_step(state: ImageState, stamp_current: bool) -> InstallStep {
    match state {
        ImageState::Unknown => InstallStep::Unknown,
        ImageState::Present if stamp_current => InstallStep::AlreadyBuilt,
        _ => InstallStep::Build,
    }
}

const UNKNOWN_WARN_EVERY_SECS: u64 = 60;
static LAST_UNKNOWN_WARN: AtomicU64 = AtomicU64::new(0);

/// Warn that the image probe failed, at most once per 60 s.
pub(super) fn warn_image_unknown() {
    let now = unix_now();
    let last = LAST_UNKNOWN_WARN.load(Ordering::SeqCst);
    if now.saturating_sub(last) >= UNKNOWN_WARN_EVERY_SECS
        && LAST_UNKNOWN_WARN
            .compare_exchange(last, now, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    {
        warn!("Diiisco: could not probe the node image — changing nothing, will re-probe");
    }
}

/// True when the stamp in `dir` is for exactly this key and pin.
pub(super) fn stamp_is_current(dir: &Path, key_fp: &str, pin_sha: &str) -> bool {
    let tag = image_tag(key_fp, pin_sha);
    read_stamp(dir).is_some_and(|s| s.tag == tag && s.key_fp == key_fp && s.pin_sha == pin_sha)
}

/// An enabled rebuild must be pinned: the `UNPINNED` placeholder (a Dockerfile
/// without a 40-hex pin) is an error, never a silent unpinned build.
pub(super) fn require_pin_when_enabled(enabled: bool, pin_sha: &str) -> Result<()> {
    if enabled && !is_hex(pin_sha, 40) {
        anyhow::bail!("Diiisco rebuild is enabled but the Dockerfile carries no 40-hex pin");
    }
    Ok(())
}

/// What the running `latest` image was built with. Written only after the new
/// node has proven itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Stamp {
    pub tag: String,
    pub key_fp: String,
    pub pin_sha: String,
}

/// One recorded rebuild attempt for `tag`. Recorded once the build has
/// finished (a failed build too) and cleared only on success; a build that a
/// stop interrupted leaves no record.
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
    let pin = field(&t, "pin_sha").filter(|v| is_hex(v, 40) || *v == UNPINNED)?;
    Some(Stamp {
        tag: tag.into(),
        key_fp: key_fp.into(),
        pin_sha: pin.into(),
    })
}

pub(super) fn read_backoff(dir: &Path) -> Option<Backoff> {
    read_backoff_file(dir, BACKOFF_FILE)
}

fn read_backoff_file(dir: &Path, file: &str) -> Option<Backoff> {
    let t = std::fs::read_to_string(dir.join(file)).ok()?;
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

/// The stamp file. `recipe_sha` is informational (the tag already folds it in);
/// `read_stamp` ignores it.
pub(super) fn write_stamp(dir: &Path, s: &Stamp) -> std::io::Result<()> {
    write_atomic(
        dir,
        STAMP_FILE,
        &format!(
            "tag={}\nkey_fp={}\npin_sha={}\nrecipe_sha={}\n",
            s.tag,
            s.key_fp,
            s.pin_sha,
            recipe_sha()
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

/// A3: record a key-proof rejection of a fresh start for `tag`; the count
/// continues only for the same target tag.
pub(super) fn record_fresh_reject(dir: &Path, tag: &str, now: u64) {
    let prev = read_backoff_file(dir, FRESH_COOLDOWN_FILE)
        .filter(|b| b.tag == tag)
        .map_or(0, |b| b.attempts);
    if let Err(e) = write_atomic(
        dir,
        FRESH_COOLDOWN_FILE,
        &format!(
            "attempts={}\nlast={now}\ntag={tag}\n",
            prev.saturating_add(1)
        ),
    ) {
        warn!(error = %e, "Diiisco: cannot record the fresh-start cooldown");
    }
}

/// A3: is a fresh start for `tag` still inside its cooldown?
pub(super) fn fresh_reject_defers(dir: &Path, tag: &str, now: u64) -> bool {
    backoff_defers(
        read_backoff_file(dir, FRESH_COOLDOWN_FILE).as_ref(),
        tag,
        now,
    )
}

/// F3: install() and start() agree. A fresh install (no image yet) whose
/// candidate did not prove the key arms the same cooldown start() reads, so a
/// start() right after is not another full build. Docker trouble and installs
/// that have an image to protect (their own backoff applies) arm nothing.
pub(super) fn note_install_reject(
    dir: &Path,
    tag: &str,
    now: u64,
    have_image: bool,
    key_not_proven: bool,
) {
    if !have_image && key_not_proven {
        record_fresh_reject(dir, tag, now);
    }
}

pub(super) fn clear_fresh_cooldown(dir: &Path) {
    if let Err(e) = std::fs::remove_file(dir.join(FRESH_COOLDOWN_FILE)) {
        if e.kind() != std::io::ErrorKind::NotFound {
            warn!(error = %e, "Diiisco: could not clear the fresh-start cooldown");
        }
    }
}

/// Put the backoff back as it was before an attempt that turned out to be a
/// user stop (a stop is not a failure): the previous record, or none.
pub(super) fn rollback_attempt(dir: &Path, prior: Option<Backoff>) {
    let Some(b) = prior else {
        clear_backoff(dir);
        return;
    };
    if let Err(e) = write_atomic(
        dir,
        BACKOFF_FILE,
        &format!(
            "attempts={}\nlast={}\ntag={}\n",
            b.attempts, b.last_unix, b.tag
        ),
    ) {
        warn!(error = %e, "Diiisco: could not restore the rebuild backoff");
    }
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

/// PURE (F2). Promotion needs the node to reject a request with no credentials
/// (401; 200 means the auth override did not load) AND to get past auth with the
/// device key: any real HTTP status other than 401/403. A post-auth 500 ("No
/// peers available" while the mesh is offline) therefore promotes; 0
/// (unreachable/timeout) on either side never does.
pub(super) fn probe_verdict(with_key_status: u16, no_key_status: u16) -> bool {
    no_key_status == 401 && with_key_status != 0 && with_key_status != 401 && with_key_status != 403
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

/// The result of an image lookup: a lookup that FAILED is not an absent image.
/// Callers keep everything on `Error` (no removal, no guard bypass).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ImageLookup {
    Present(String),
    Absent,
    Error,
}

impl ImageLookup {
    /// The id when the image is known to exist; `Absent` and `Error` are `None`.
    pub(super) fn into_id(self) -> Option<String> {
        match self {
            ImageLookup::Present(id) => Some(id),
            ImageLookup::Absent | ImageLookup::Error => None,
        }
    }
}

/// Everything the sequence needs from Docker. Output handed back in errors is
/// already redacted.
#[async_trait]
pub(super) trait NodeOps: Send + Sync {
    fn image_exists(&self) -> bool;
    /// Tri-state probe; the default maps `image_exists`.
    fn image_state(&self) -> ImageState {
        if self.image_exists() {
            ImageState::Present
        } else {
            ImageState::Absent
        }
    }
    /// Put this build's embedded Dockerfile on disk. Called before every
    /// build: only install() used to write it, so an upgraded device would
    /// otherwise build from a stale (unpinned) copy.
    fn refresh_dockerfile(&self) -> Result<()>;
    fn image_id(&self, reference: &str) -> Option<String>;
    /// Tri-state lookup; the default maps `image_id` (no error channel).
    fn image_lookup(&self, reference: &str) -> ImageLookup {
        match self.image_id(reference) {
            Some(id) => ImageLookup::Present(id),
            None => ImageLookup::Absent,
        }
    }
    /// `compose build` with `DIIISCO_IMAGE_TAG=<tag>`.
    fn build(&self, tag: &str) -> Result<()>;
    fn down(&self);
    /// The existing `compose up -d` (image resolved from `latest`).
    fn up_plain(&self) -> Result<()>;
    /// `compose up -d --no-build`; `Some(tag)` sets the tag, `None` removes it.
    fn up_no_build(&self, tag: Option<&str>) -> Result<()>;
    /// The AM3 auth probe, with bounded retries while the node boots.
    async fn probe(&self) -> bool;
    /// Did the last candidate get as far as the key probe? A rejection after
    /// the probe is a verdict on the device key; one before it is Docker
    /// trouble. Default `false`: no cooldown is armed on a guess.
    fn probe_ran(&self) -> bool {
        false
    }
    fn tag_latest(&self, tag: &str) -> Result<()>;
    /// Point `diiisco-node:latest` back at image `id` (undoing `tag_latest`).
    /// Default: nothing; Docker-backed impls override.
    fn retag_latest(&self, _id: &str) -> Result<()> {
        Ok(())
    }
    fn rmi_by_id(&self, id: &str);
    /// Image ids of ALL containers, running or stopped; `None` = unknown.
    fn all_container_image_ids(&self) -> Option<Vec<String>> {
        Some(Vec::new())
    }
    /// Images of repo `diiisco-node` with their tags; `None` = unknown.
    fn list_node_images(&self) -> Option<Vec<ImageRef>> {
        Some(Vec::new())
    }
    /// Count of dangling (untagged) images, for the log only.
    fn dangling_image_count(&self) -> Option<usize> {
        Some(0)
    }
    /// True when the image's history shows it was built from FEM's Dockerfile.
    fn image_built_by_fem(&self, _id: &str) -> bool {
        false
    }
    /// Record the id of the just-built candidate `tag` in the candidate
    /// ledger and return it. Default: nothing recorded, no id.
    fn record_candidate(&self, _tag: &str) -> Option<String> {
        None
    }
    /// A candidate image was removed: drop it from the ledger.
    fn forget_candidate(&self, _id: &str) {}
    /// The tag recorded in the current image stamp, if any.
    fn stamp_tag(&self) -> Option<String> {
        None
    }
    /// Re-point `diiisco-node:<tag>` at image `id`. Default: nothing.
    fn retag_alias(&self, _tag: &str, _id: &str) -> Result<()> {
        Ok(())
    }
    /// `rmi_by_id` with a name for what is being removed, used in the log
    /// line; true when the image was removed. Default: ignores the name and
    /// assumes success (`rmi_by_id` has no result).
    fn rmi_labeled(&self, id: &str, _what: &str) -> bool {
        self.rmi_by_id(id);
        true
    }
    /// Remove the candidate image `diiisco-node:<tag>` after a failed or
    /// interrupted attempt. Default: nothing; Docker-backed impls override.
    fn discard_candidate(&self, _tag: &str) {}
    /// Image IDs used by RUNNING containers; `None` = the probe failed, so a
    /// caller must not remove anything. Default: no running containers.
    fn running_image_ids(&self) -> Option<Vec<String>> {
        Some(Vec::new())
    }
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
    /// The image probe failed: nothing was built, planned, recorded or changed.
    ImageUnknown,
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
    clear_fresh_cooldown(dir);
    Ok(())
}

/// One image of repo `diiisco-node`: its id and its tag (`None` = untagged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ImageRef {
    pub id: String,
    pub tag: Option<String>,
}

pub(super) const LEDGER_FILE: &str = "candidate-ids.ledger";

pub(super) fn ledger_read(dir: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(dir.join(LEDGER_FILE)) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for line in text.lines().map(str::trim) {
        if is_image_id(line) && !out.iter().any(|x| x == line) {
            out.push(line.to_string());
        }
    }
    out
}

/// `sha256:` plus exactly 64 lowercase hex digits: the only shape a ledger
/// line or a listed id may have (it later reaches an argv, so nothing else is
/// accepted).
fn is_image_id(s: &str) -> bool {
    s.strip_prefix("sha256:").is_some_and(|h| is_hex(h, 64))
}

fn write_ledger(dir: &Path, ids: &[String]) -> std::io::Result<()> {
    let mut text = ids.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    write_atomic(dir, LEDGER_FILE, &text)
}

pub(super) fn ledger_add(dir: &Path, id: &str) -> std::io::Result<()> {
    if !is_image_id(id) {
        return Ok(());
    }
    let mut ids = ledger_read(dir);
    if ids.iter().any(|x| x == id) {
        return Ok(());
    }
    ids.push(id.to_string());
    if ids.len() > LEDGER_MAX {
        ids.drain(..ids.len() - LEDGER_MAX);
    }
    write_ledger(dir, &ids)
}

pub(super) fn ledger_remove(dir: &Path, id: &str) {
    let ids = ledger_read(dir);
    if !ids.iter().any(|x| x == id) {
        return;
    }
    let kept: Vec<String> = ids.into_iter().filter(|x| x != id).collect();
    if let Err(e) = write_ledger(dir, &kept) {
        warn!(error = %e, "Diiisco: could not update the candidate ledger");
    }
}

/// Parse `docker images ... --format "{{.ID}} {{.Tag}}"` output.
pub(super) fn parse_image_listing(text: &str) -> Vec<ImageRef> {
    text.lines()
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let (id, tag) = (parts.next()?, parts.next()?);
            (parts.next().is_none() && is_image_id(id)).then(|| ImageRef {
                id: id.to_string(),
                tag: (tag != "<none>").then(|| tag.to_string()),
            })
        })
        .collect()
}

/// True when a `docker history` dump shows FEM's Dockerfile built the image.
pub(super) fn history_is_fem(text: &str) -> bool {
    FEM_HISTORY_MARKS.iter().all(|m| text.contains(m))
}

/// Lines of the final stage of FEM's node Dockerfile, identical in every FEM
/// Dockerfile since v0.4.37 (`docker history` never shows the builder stage):
/// the `/build/` copy source, the data dir and the entry point together.
const FEM_HISTORY_MARKS: [&str; 3] = ["/build/node_modules", "mkdir -p /app/data", "dist/dev.js"];

/// At most this many candidate ids are remembered (oldest dropped).
const LEDGER_MAX: usize = 64;

/// What the sweep decides from.
pub(super) struct SweepSets<'a> {
    pub ledger: &'a [String],
    pub images: &'a [ImageRef],
    pub containers: &'a [String],
    pub running: &'a [String],
    pub new_id: &'a str,
}

/// PURE. The image ids a successful promotion may remove.
///
/// Never the promoted image, the image of any container (running or stopped),
/// or an image carrying the `latest` tag. Then either a ledger id (a candidate
/// this FEM built) or a legacy candidate: every tag of the id is
/// `fem-<12 hex>` (repo `diiisco-node`, as listed) and its history shows FEM's
/// Dockerfile. An id with any other tag is somebody's image and stays.
pub(super) fn sweep_targets(s: &SweepSets, fem_built: &dyn Fn(&str) -> bool) -> Vec<String> {
    let tags_of = |id: &str| -> Vec<Option<&str>> {
        s.images
            .iter()
            .filter(|i| i.id == id)
            .map(|i| i.tag.as_deref())
            .collect()
    };
    let protected = |id: &str| {
        id == s.new_id
            || s.containers.iter().any(|c| c == id)
            || s.running.iter().any(|c| c == id)
            || tags_of(id).contains(&Some("latest"))
    };
    let all_fem_tags = |id: &str| tags_of(id).iter().all(|t| t.is_some_and(is_tag));
    let mut out: Vec<String> = Vec::new();
    let mut take = |id: &str| {
        if !out.iter().any(|x| x == id) {
            out.push(id.to_string());
        }
    };
    for id in s.ledger {
        if is_image_id(id) && !protected(id) && tags_of(id).iter().all(|t| t.is_none_or(is_tag)) {
            take(id);
        }
    }
    for img in s.images {
        let id = img.id.as_str();
        if is_image_id(id) && !protected(id) && all_fem_tags(id) && fem_built(id) {
            take(id);
        }
    }
    out
}

/// After a successful promotion: remove stale FEM-created candidate images.
///
/// Fail-closed: without the promoted image's id, or with any unknown listing
/// (images, containers, running), nothing is removed. Dangling unlabeled
/// images cannot be attributed to FEM and are only counted. Removal is by id,
/// through the one `rmi_labeled` path; there is no prune.
pub(super) fn sweep_stale_candidates<O: NodeOps + ?Sized>(
    ops: &O,
    dir: &Path,
    new_id: Option<&str>,
) {
    let Some(new_id) = new_id else {
        return;
    };
    // The promoted image is no longer a candidate.
    ledger_remove(dir, new_id);
    let (Some(running), Some(containers), Some(images)) = (
        ops.running_image_ids(),
        ops.all_container_image_ids(),
        ops.list_node_images(),
    ) else {
        warn!("Diiisco: stale-candidate sweep skipped (Docker state unknown)");
        return;
    };
    let ledger = ledger_read(dir);
    let sets = SweepSets {
        ledger: &ledger,
        images: &images,
        containers: &containers,
        running: &running,
        new_id,
    };
    for id in sweep_targets(&sets, &|id| ops.image_built_by_fem(id)) {
        // Only a removal that happened takes the id off the ledger.
        if ops.rmi_labeled(&id, "stale candidate image") {
            ledger_remove(dir, &id);
        }
    }
    // Counted after the removals: a dangling ledger candidate that was just
    // removed is not "left in place".
    if let Some(n) = ops.dangling_image_count().filter(|n| *n > 0) {
        info!("Diiisco: {n} dangling image(s) left in place (not attributable to FEM)");
    }
}

/// The log line for one image removal: it names WHAT was removed (the previous
/// node image, a failed candidate, a stale candidate), never a fixed phrase.
pub(super) fn rmi_log_text(what: &str, removed: bool) -> String {
    if removed {
        format!("Diiisco: removed the {what}")
    } else {
        format!("Diiisco: {what} not removed (continuing)")
    }
}

/// Undo `tag_latest` after a promotion failed later (the stamp could not be
/// written): `latest` goes back to the previous image so the node that is
/// restored is the old, proven one. No previous image = nothing to restore.
pub(super) fn rollback_latest<O: NodeOps + ?Sized>(ops: &O, prev: Option<&str>) {
    let Some(prev) = prev else {
        return;
    };
    if let Err(e) = ops.retag_latest(prev) {
        warn!(error = %e, "Diiisco: could not point latest back at the previous image");
    }
}

/// The id to remove for a failed candidate, or `None` to keep it. Only an image
/// that exists, is not `latest`, and is not used by a running container goes;
/// an unknown container list (`None`) keeps everything.
pub(super) fn candidate_to_remove(
    cand: Option<&str>,
    latest: Option<&str>,
    running: Option<&[String]>,
) -> Option<String> {
    let cand = cand?;
    if latest == Some(cand) {
        return None;
    }
    if running?.iter().any(|r| r == cand) {
        return None;
    }
    Some(cand.to_string())
}

/// Why a failed candidate image was kept (L7): when it IS the current image
/// the log says so, instead of calling the node's own image a failed candidate
/// kept for a vague reason.
pub(super) fn kept_reason(cand: Option<&str>, latest: Option<&str>) -> &'static str {
    if cand.is_some() && cand == latest {
        "it is the current image"
    } else {
        "in use, or containers unknown"
    }
}

/// Remove the failed candidate `diiisco-node:<tag>` BY ID, never by tag pattern
/// and never `latest`. Call it only after the candidate's container was stopped
/// or replaced.
pub(super) fn discard_candidate_by_id<O: NodeOps + ?Sized>(ops: &O, tag: &str) {
    let cand = ops.image_lookup(&format!("diiisco-node:{tag}"));
    let latest = ops.image_lookup(LATEST_REF);
    if cand == ImageLookup::Error || latest == ImageLookup::Error {
        // A failed lookup is not an absent image: keep everything.
        warn!("Diiisco: failed candidate image kept (image lookup failed)");
        return;
    }
    let (cand, latest) = (cand.into_id(), latest.into_id());
    let running = ops.running_image_ids();
    match candidate_to_remove(cand.as_deref(), latest.as_deref(), running.as_deref()) {
        Some(id) => {
            if !ops.rmi_labeled(&id, "failed candidate image") {
                // Still there: keep its ledger entry and leave fem-<tag> where it is.
                warn!("Diiisco: failed candidate image not removed (kept on the ledger)");
                return;
            }
            ops.forget_candidate(&id);
            // A rebuild of the stamped tag re-points `diiisco-node:<tag>` at
            // the candidate; removing the candidate by id takes that alias
            // with it, so give `latest` its alias back.
            if ops.stamp_tag().as_deref() == Some(tag) {
                if let Some(latest) = latest.as_deref() {
                    if let Err(e) = ops.retag_alias(tag, latest) {
                        warn!(error = %e, "Diiisco: could not restore the image alias");
                    }
                }
            }
        }
        None if cand.is_some() => {
            warn!(
                "Diiisco: failed candidate image kept ({})",
                kept_reason(cand.as_deref(), latest.as_deref())
            );
        }
        None => {}
    }
}

/// Old image back up after a failed or interrupted upgrade. `latest` was never
/// moved, so compose resolves it without any tag env. The failed candidate, if
/// one was built, is discarded once the restore has been attempted.
fn fallback_old<O: NodeOps + ?Sized>(ops: &O, cand: Option<&str>) -> Result<Outcome> {
    if ops.stop_requested() {
        info!("Diiisco: stopped during rebuild — leaving the node down");
        if let Some(t) = cand {
            stopped_cleanup(ops, t);
        }
        return Ok(Outcome::Stopped);
    }
    let restored = ops.up_no_build(None);
    if let Some(t) = cand {
        ops.discard_candidate(t);
    }
    restored?;
    Ok(Outcome::UpgradeFailed)
}

/// What the candidate image proved when brought up for the F2 gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Gate {
    /// Up, rejected a request without a key, accepted the device key.
    Pass,
    /// Failed to start or to prove the key; the previous node is restored and
    /// the candidate discarded.
    Rejected,
    /// `stop()` ran meanwhile; nothing was promoted.
    Stopped,
}

/// Bring the built candidate `tag` up and run the F2 probe on it. The caller
/// holds the rebuild lock; nothing here locks, so it is safe under it.
pub(super) async fn gate_candidate<O: NodeOps + ?Sized>(
    ops: &O,
    tag: &str,
    have_image: bool,
) -> Gate {
    if ops.stop_requested() {
        ops.discard_candidate(tag);
        return Gate::Stopped;
    }
    if have_image {
        ops.down();
        if ops.stop_requested() {
            ops.discard_candidate(tag);
            return Gate::Stopped;
        }
    }
    if let Err(e) = ops.up_no_build(Some(tag)) {
        if ops.stop_requested() {
            stopped_cleanup(ops, tag);
            return Gate::Stopped;
        }
        warn!(error = %e, "Diiisco: the new node failed to start");
        reject_candidate(ops, tag, have_image);
        return Gate::Rejected;
    }
    // The real probe returns false when it sees a stop, so a false probe with
    // a stop pending is a stop, not a rejection (no backoff, no error).
    let proved = ops.probe().await;
    if ops.stop_requested() {
        stopped_cleanup(ops, tag);
        return Gate::Stopped;
    }
    if !proved {
        warn!("Diiisco: the new node did not prove the device key");
        reject_candidate(ops, tag, have_image);
        return Gate::Rejected;
    }
    Gate::Pass
}

/// A stop is pending and the candidate's container may still exist (`stop()`
/// only stops it): remove the container, then the candidate image by id, and
/// bring nothing up. Without the `down` the stopped container keeps the image
/// and the removal conflicts (the K1 leak).
fn stopped_cleanup<O: NodeOps + ?Sized>(ops: &O, tag: &str) {
    ops.down();
    ops.discard_candidate(tag);
}

/// Undo a rejected candidate: remove its container, restore the previous node
/// when there was one and no stop is pending, then discard the candidate image
/// by id. After a `stop()` nothing is brought up.
pub(super) fn reject_candidate<O: NodeOps + ?Sized>(ops: &O, tag: &str, have_image: bool) {
    ops.down();
    if have_image && !ops.stop_requested() {
        if let Err(e) = ops.up_no_build(None) {
            warn!(error = %e, "Diiisco: could not restore the previous node");
        }
    }
    ops.discard_candidate(tag);
}

/// After a promotion: remove the PREVIOUS `latest` image by id, unless it is
/// the new image itself.
pub(super) fn drop_previous<O: NodeOps + ?Sized>(ops: &O, prev: Option<&str>, tag: &str) {
    let Some(prev) = prev else {
        return;
    };
    if previous_to_remove(ops, prev, tag) {
        ops.rmi_labeled(prev, "previous node image");
    }
}

/// True when the PREVIOUS image may go: the new image's id is known and
/// differs from it. A failed lookup keeps the previous image.
fn previous_to_remove<O: NodeOps + ?Sized>(ops: &O, prev: &str, tag: &str) -> bool {
    match ops.image_lookup(&format!("diiisco-node:{tag}")) {
        ImageLookup::Error => {
            warn!("Diiisco: previous node image kept (image lookup failed)");
            false
        }
        found => found.into_id().as_deref() != Some(prev),
    }
}

/// PURE. True when an attempt for `tag` is recorded and its delay has not
/// elapsed. A backoff stamped in the future is ignored.
pub(super) fn backoff_defers(backoff: Option<&Backoff>, tag: &str, now: u64) -> bool {
    backoff
        .filter(|b| b.tag == tag)
        .and_then(|b| {
            now.checked_sub(b.last_unix)
                .map(|el| el < delay_secs(b.attempts))
        })
        .unwrap_or(false)
}

/// install()'s backoff check, run under the rebuild lock before any build: a
/// failed attempt for this target is not retried by every recovery cycle.
pub(super) fn install_backoff_check(dir: &Path, tag: &str, now: u64) -> Result<()> {
    if backoff_defers(read_backoff(dir).as_ref(), tag, now) {
        anyhow::bail!(
            "Diiisco node image rebuild is backing off after a failed attempt; will retry later"
        );
    }
    Ok(())
}

/// Record the attempt of a build that failed (or could not start); a failure
/// to record only warns.
fn note_failed_attempt(dir: &Path, tag: &str, now: u64) {
    if let Err(e) = record_attempt(dir, tag, now) {
        warn!(error = %e, "Diiisco: cannot record the failed rebuild attempt");
    }
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
    rebuild_enabled: bool,
) -> Result<Outcome> {
    require_pin_when_enabled(rebuild_enabled, pin_sha)?;
    let _guard = REBUILD_LOCK.lock().await;
    let now = clock();
    let have_image = match ops.image_state() {
        ImageState::Present => true,
        ImageState::Absent => false,
        // Never build, never touch the backoff or the stamp, leave the
        // containers as they are; the next supervisor tick re-probes.
        ImageState::Unknown => {
            warn_image_unknown();
            return Ok(Outcome::ImageUnknown);
        }
    };
    let plan = plan_start(
        read_stamp(dir).as_ref(),
        read_backoff(dir).as_ref(),
        key_fp,
        pin_sha,
        now,
    );
    let gated = gate_plan(plan.clone(), rebuild_enabled, have_image);
    if gated != plan {
        info!(
            "Diiisco: node image upgrade disabled (BLOCKER-SANDBOX) — running the existing image"
        );
    }
    let plan = gated;
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
        // A3: a rejected fresh node is not rebuilt by every cycle. Read only
        // here: the upgrade path has its own backoff and a node to protect.
        if fresh_reject_defers(dir, &tag, now) {
            anyhow::bail!(
                "Diiisco node image build is backing off after a rejected fresh start; will retry later"
            );
        }
        info!("Diiisco: no node image — building before start");
        ops.refresh_dockerfile()?;
        if let Err(e) = ops.build(&tag) {
            ops.discard_candidate(&tag);
            return Err(e);
        }
        let cand_id = ops.record_candidate(&tag);
        if ops.stop_requested() {
            ops.discard_candidate(&tag);
            return Ok(Outcome::Stopped);
        }
        match gate_candidate(ops, &tag, false).await {
            Gate::Pass => {
                if let Err(e) = after_fresh_build(ops, dir, &tag, key_fp, pin_sha) {
                    // The proven candidate is still running: remove it too.
                    stopped_cleanup(ops, &tag);
                    if let Err(re) = record_attempt(dir, &tag, now) {
                        warn!(error = %re, "Diiisco: cannot record rebuild attempt after a failed promotion");
                    }
                    return Err(e);
                }
            }
            Gate::Rejected => {
                if let Err(e) = record_attempt(dir, &tag, now) {
                    warn!(error = %e, "Diiisco: cannot record rebuild attempt after a rejected fresh node");
                }
                if ops.probe_ran() {
                    record_fresh_reject(dir, &tag, now);
                }
                anyhow::bail!(
                    "the new Diiisco node did not prove the device key on start; nothing was promoted"
                );
            }
            Gate::Stopped => return Ok(Outcome::Stopped),
        }
        ops.down();
        if ops.stop_requested() {
            return Ok(Outcome::Stopped);
        }
        ops.up_plain()?;
        sweep_stale_candidates(ops, dir, cand_id.as_deref());
        return Ok(Outcome::BuiltFresh);
    }

    // Upgrade: the old container keeps running while the new image builds.
    let prev_id = ops.image_lookup(LATEST_REF).into_id();
    // The attempt is recorded once the build has finished (like install()):
    // a build interrupted by a stop leaves no backoff, a failed one does.
    let prior_backoff = read_backoff(dir);
    // A stop that lands after the attempt was recorded is not a failure.
    let unless_stopped = |out: Result<Outcome>| {
        if matches!(out, Ok(Outcome::Stopped)) {
            rollback_attempt(dir, prior_backoff.clone());
        }
        out
    };
    if let Err(e) = ops.refresh_dockerfile() {
        warn!(error = %e, "Diiisco: cannot refresh the node Dockerfile — keeping the current node");
        note_failed_attempt(dir, &tag, now);
        return fallback_old(ops, None);
    }
    info!(fp = %key_fp, "Diiisco: rebuilding node image for the per-device key");
    if let Err(e) = ops.build(&tag) {
        warn!(error = %e, "Diiisco: upgrade build failed — keeping the current node");
        note_failed_attempt(dir, &tag, now);
        return fallback_old(ops, Some(tag.as_str()));
    }
    let cand_id = ops.record_candidate(&tag);
    if ops.stop_requested() {
        info!("Diiisco: stopped during rebuild — not switching");
        ops.discard_candidate(&tag);
        return Ok(Outcome::Stopped);
    }
    if let Err(e) = record_attempt(dir, &tag, now) {
        warn!(error = %e, "Diiisco: cannot record rebuild attempt — keeping the current node");
        ops.discard_candidate(&tag);
        return fallback_old(ops, None);
    }
    ops.down();
    if ops.stop_requested() {
        ops.discard_candidate(&tag);
        return unless_stopped(Ok(Outcome::Stopped));
    }
    if let Err(e) = ops.up_no_build(Some(&tag)) {
        warn!(error = %e, "Diiisco: new node failed to start — restoring the current node");
        return unless_stopped(fallback_old(ops, Some(tag.as_str())));
    }
    if !ops.probe().await {
        warn!("Diiisco: new node did not prove the device key — restoring the current node");
        return unless_stopped(fallback_old(ops, Some(tag.as_str())));
    }
    if ops.stop_requested() {
        stopped_cleanup(ops, &tag);
        return unless_stopped(Ok(Outcome::Stopped));
    }
    if let Err(e) = ops.tag_latest(&tag) {
        warn!(error = %e, "Diiisco: could not promote the new image — restoring the current node");
        return unless_stopped(fallback_old(ops, Some(tag.as_str())));
    }
    let stamp = Stamp {
        tag: tag.clone(),
        key_fp: key_fp.to_string(),
        pin_sha: pin_sha.to_string(),
    };
    if let Err(e) = write_stamp(dir, &stamp) {
        // The new image is `latest` but unstamped: undo the promotion so the
        // restored node, the stamp and `latest` agree, and discard the
        // candidate (the backoff stays, so the retry waits).
        warn!(error = %e, "Diiisco: could not write the image stamp — restoring the current node");
        rollback_latest(ops, prev_id.as_deref());
        let cand = Some(tag.as_str());
        return unless_stopped(fallback_old(ops, cand));
    }
    clear_backoff(dir);
    clear_fresh_cooldown(dir);
    if let Some(prev) = prev_id.filter(|p| previous_to_remove(ops, p, &tag)) {
        ops.rmi_labeled(&prev, "previous node image");
    }
    sweep_stale_candidates(ops, dir, cand_id.as_deref());
    Ok(Outcome::Upgraded)
}
