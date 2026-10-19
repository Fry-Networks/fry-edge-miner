//! G1 (rc39b): A6 on the start() path. A copy of the A4 fake (that
//! module's fake is private). Synthetic values only.
#![allow(dead_code)]
use super::super::diiisco_rebuild::{
    fp12, image_tag, ledger_add, ledger_remove, read_stamp, record_attempt, run_node_sequence,
    ImageLookup, ImageRef, NodeOps, Outcome, Stamp, LATEST_REF,
};
use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const NOW: u64 = 2_000_000;
const OLD: &str = "sha256:cba06b5736faf67e54b07b561eae94395e774c517a7d910a54369e1263ccfbd4";
const NEW: &str = "sha256:11507a0e2f5e69d5dfa40a62a1bd7b6ee57e6bcd85c67c9b8431b36fff21c437";

fn key_fp() -> String {
    fp12(&format!("sk-fem-{}", "ab".repeat(32)))
}

fn pin() -> String {
    "a".repeat(40)
}

fn tag() -> String {
    image_tag(&key_fp(), &pin())
}

struct R {
    dir: PathBuf,
    calls: Mutex<Vec<String>>,
    present: AtomicBool,
    cand_exists: AtomicBool,
    promoted: AtomicBool,
    /// Image id of the (single) node container; `down` removes it.
    container: Mutex<Option<String>>,
    stop: AtomicBool,
    stop_on_probe: bool,
    stop_on_up: bool,
    probe_ok: bool,
    up_ok: bool,
    tag_ok: bool,
    stop_on_build: bool,
    build_ok: bool,
    /// The tag in the current image stamp.
    stamp_tag: Option<String>,
    /// `diiisco-node` images as listed (`None` = the listing failed).
    images: Option<Vec<ImageRef>>,
    /// The running-container list is unknown (docker failed).
    running_unknown: bool,
    /// Ids whose history shows FEM's Dockerfile.
    fem_ids: Vec<String>,
    /// Ids whose `docker rmi` fails.
    rmi_fail_ids: Vec<String>,
    /// Images held by STOPPED containers other than the node container.
    stopped_containers: Vec<String>,
    /// `docker ps -a` / inspect failed.
    containers_unknown: bool,
    /// Dangling (untagged) images; the count drops as they are removed.
    dangling_ids: Vec<String>,
    /// References whose lookup FAILS (docker error), as opposed to absent.
    error_refs: Vec<String>,
    /// Every reference other than `latest` resolves to the same image as `latest`
    /// (`diiisco-node:<tag>` is an alias of it).
    alias_is_latest: bool,
    /// `docker tag` for an alias fails.
    retag_alias_fails: bool,
    /// References whose FIRST lookup fails (a transient docker error).
    error_once: Mutex<Vec<String>>,
    /// The stamp file exists but cannot be read.
    stamp_unreadable: bool,
}

impl R {
    fn new(dir: &Path, have_image: bool) -> Self {
        R {
            dir: dir.to_path_buf(),
            calls: Mutex::new(Vec::new()),
            present: AtomicBool::new(have_image),
            cand_exists: AtomicBool::new(false),
            promoted: AtomicBool::new(false),
            container: Mutex::new(have_image.then(|| OLD.to_string())),
            stop: AtomicBool::new(false),
            stop_on_probe: false,
            stop_on_up: false,
            probe_ok: true,
            up_ok: true,
            tag_ok: true,
            stop_on_build: false,
            build_ok: true,
            stamp_tag: None,
            images: Some(Vec::new()),
            running_unknown: false,
            fem_ids: Vec::new(),
            rmi_fail_ids: Vec::new(),
            stopped_containers: Vec::new(),
            containers_unknown: false,
            dangling_ids: Vec::new(),
            error_refs: Vec::new(),
            alias_is_latest: false,
            retag_alias_fails: false,
            error_once: Mutex::new(Vec::new()),
            stamp_unreadable: false,
        }
    }

    /// One `docker rmi <id>`: false when a container still uses the image or
    /// the id is in `rmi_fail_ids`.
    fn try_rmi(&self, id: &str) -> bool {
        let used = self.container.lock().unwrap().as_deref() == Some(id)
            || self.stopped_containers.iter().any(|c| c == id);
        if used {
            self.rec(format!("rmi_conflict:{id}"));
            return false;
        }
        if self.rmi_fail_ids.iter().any(|f| f == id) {
            self.rec(format!("rmi_failed:{id}"));
            return false;
        }
        self.rec(format!("rmi:{id}"));
        if id == NEW {
            self.cand_exists.store(false, Ordering::SeqCst);
        }
        true
    }

    fn rec(&self, c: String) {
        self.calls.lock().unwrap().push(c);
    }

    fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| !c.starts_with("image_id:"))
            .cloned()
            .collect()
    }

    fn has_prefix(&self, p: &str) -> bool {
        self.calls().iter().any(|c| c.starts_with(p))
    }
}

#[async_trait]
impl NodeOps for R {
    fn image_exists(&self) -> bool {
        self.present.load(Ordering::SeqCst)
    }
    fn refresh_dockerfile(&self) -> Result<()> {
        Ok(())
    }
    fn image_id(&self, reference: &str) -> Option<String> {
        self.rec(format!("image_id:{reference}"));
        if reference == LATEST_REF {
            if !self.present.load(Ordering::SeqCst) {
                return None;
            }
            return Some(
                if self.promoted.load(Ordering::SeqCst) {
                    NEW
                } else {
                    OLD
                }
                .into(),
            );
        }
        if self.alias_is_latest {
            return self.present.load(Ordering::SeqCst).then(|| OLD.into());
        }
        self.cand_exists.load(Ordering::SeqCst).then(|| NEW.into())
    }
    fn image_lookup(&self, reference: &str) -> ImageLookup {
        let once = {
            let mut v = self.error_once.lock().unwrap();
            v.iter()
                .position(|r| r == reference)
                .map(|i| v.remove(i))
                .is_some()
        };
        if once || self.error_refs.iter().any(|r| r == reference) {
            self.rec(format!("image_id:{reference}"));
            return ImageLookup::Error;
        }
        match self.image_id(reference) {
            Some(id) => ImageLookup::Present(id),
            None => ImageLookup::Absent,
        }
    }
    fn build(&self, t: &str) -> Result<()> {
        self.rec(format!("build:{t}"));
        self.cand_exists.store(true, Ordering::SeqCst);
        if self.stop_on_build {
            self.stop.store(true, Ordering::SeqCst);
        }
        if !self.build_ok {
            anyhow::bail!("fake build failed");
        }
        Ok(())
    }
    fn down(&self) {
        self.rec("down".into());
        *self.container.lock().unwrap() = None;
    }
    fn up_plain(&self) -> Result<()> {
        self.rec("up_plain".into());
        let id = if self.promoted.load(Ordering::SeqCst) {
            NEW
        } else {
            OLD
        };
        *self.container.lock().unwrap() = Some(id.into());
        Ok(())
    }
    fn up_no_build(&self, t: Option<&str>) -> Result<()> {
        self.rec(format!("up_no_build:{}", t.unwrap_or("none")));
        match t {
            Some(_) => {
                // a partial create can leave the candidate container behind
                *self.container.lock().unwrap() = Some(NEW.into());
                if self.stop_on_up {
                    self.stop.store(true, Ordering::SeqCst);
                }
                if !self.up_ok {
                    anyhow::bail!("fake up failed");
                }
            }
            None => *self.container.lock().unwrap() = Some(OLD.into()),
        }
        Ok(())
    }
    fn probe_ran(&self) -> bool {
        self.calls().iter().any(|c| c == "probe")
    }
    async fn probe(&self) -> bool {
        self.rec("probe".into());
        if self.stop_on_probe {
            self.stop.store(true, Ordering::SeqCst);
        }
        self.probe_ok
    }
    fn tag_latest(&self, t: &str) -> Result<()> {
        self.rec(format!("tag_latest:{t}"));
        if !self.tag_ok {
            anyhow::bail!("fake tag failed");
        }
        self.promoted.store(true, Ordering::SeqCst);
        self.present.store(true, Ordering::SeqCst);
        Ok(())
    }
    /// Every container, running or stopped: the node container plus the stopped ones.
    fn all_container_image_ids(&self) -> Option<Vec<String>> {
        if self.containers_unknown {
            return None;
        }
        let mut ids: Vec<String> = self.container.lock().unwrap().clone().into_iter().collect();
        ids.extend(self.stopped_containers.iter().cloned());
        Some(ids)
    }
    /// Dangling images still present: the ones not yet removed by an rmi.
    fn dangling_image_count(&self) -> Option<usize> {
        let removed = self.calls();
        let left = self
            .dangling_ids
            .iter()
            .filter(|d| !removed.contains(&format!("rmi:{d}")))
            .count();
        self.rec(format!("dangling_count:{left}"));
        Some(left)
    }
    fn list_node_images(&self) -> Option<Vec<ImageRef>> {
        self.images.clone()
    }
    fn image_built_by_fem(&self, id: &str) -> bool {
        self.fem_ids.iter().any(|f| f == id)
    }
    fn record_candidate(&self, t: &str) -> Option<String> {
        self.rec(format!("record_candidate:{t}"));
        ledger_add(&self.dir, NEW).ok()?;
        Some(NEW.into())
    }
    fn forget_candidate(&self, id: &str) {
        ledger_remove(&self.dir, id);
    }
    fn stamp_tag(&self) -> Option<String> {
        self.stamp_tag.clone()
    }
    fn stamp_unreadable(&self) -> bool {
        self.stamp_unreadable
    }
    fn note_alias_restored(&self, t: &str) -> bool {
        super::super::diiisco_rebuild::write_alias_marker(&self.dir, t)
    }
    fn forget_alias_restored(&self) {
        super::super::diiisco_rebuild::clear_alias_marker(&self.dir);
    }
    fn retag_alias(&self, t: &str, id: &str) -> Result<()> {
        self.rec(format!("retag_alias:{t}:{id}"));
        if self.retag_alias_fails {
            anyhow::bail!("fake tag failed");
        }
        Ok(())
    }
    fn retag_latest(&self, id: &str) -> Result<()> {
        self.rec(format!("retag_latest:{id}"));
        self.promoted.store(id != OLD, Ordering::SeqCst);
        Ok(())
    }
    fn rmi_labeled(&self, id: &str, what: &str) -> bool {
        self.rec(format!("rmi_label:{what}"));
        self.try_rmi(id)
    }
    fn rmi_by_id(&self, id: &str) {
        self.try_rmi(id);
    }
    fn discard_candidate(&self, t: &str) {
        super::super::diiisco_rebuild::discard_candidate_by_id(self, t);
    }
    /// `compose stop` leaves a stopped container: not running, still present.
    fn running_image_ids(&self) -> Option<Vec<String>> {
        if self.running_unknown {
            return None;
        }
        let c = self.container.lock().unwrap().clone();
        Some(if self.stop.load(Ordering::SeqCst) {
            Vec::new()
        } else {
            c.into_iter().collect()
        })
    }
    fn reuse_unstamped_image(&self) -> bool {
        true
    }
    fn stop_requested(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

async fn seq(r: &R) -> Result<Outcome> {
    run_node_sequence(r, &r.dir, &key_fp(), &pin(), || NOW, true).await
}

/// Index of the first `what`; a missing call is `usize::MAX`, so an ordering
/// assertion fails instead of the test panicking.
fn at(c: &[String], what: &str) -> usize {
    c.iter().position(|x| x == what).unwrap_or(usize::MAX)
}

/// Index of the last `what`; a missing call is 0.
fn last(c: &[String], what: &str) -> usize {
    c.iter().rposition(|x| x == what).unwrap_or(0)
}

// ------------------------------------------------------------------------ F3

// ------------------------------------------------------------------------ G1
// A node whose image is present but unstamped used to be REBUILT by start()
// (plan_start has no image argument). Now the existing latest is proven with
// the F2 probe and stamped instead; only a probe pass stamps.

fn unstamped_with_alias(d: &Path) -> R {
    let mut r = R::new(d, true);
    r.alias_is_latest = true;
    r
}

#[tokio::test]
async fn g1_start_with_an_unstamped_matching_image_reuses_it_without_a_build() {
    let d = tempfile::tempdir().unwrap();
    let r = unstamped_with_alias(d.path());
    let out = seq(&r).await;
    let c = r.calls();
    assert_eq!(out.ok(), Some(Outcome::Ran), "{c:?}");
    assert_eq!(
        c.iter().filter(|x| x.starts_with("build:")).count(),
        0,
        "{c:?}"
    );
    assert_eq!(c.iter().filter(|x| *x == "probe").count(), 1, "{c:?}");
    assert_eq!(
        read_stamp(d.path()),
        Some(Stamp {
            tag: tag(),
            key_fp: key_fp(),
            pin_sha: pin()
        })
    );
    assert!(
        at(&c, "probe") < at(&c, &format!("tag_latest:{}", tag())),
        "{c:?}"
    );
}

#[tokio::test]
async fn g1_the_m4_marker_still_builds() {
    let d = tempfile::tempdir().unwrap();
    super::super::diiisco_rebuild::write_alias_marker(d.path(), &tag());
    let r = unstamped_with_alias(d.path());
    let out = seq(&r).await;
    let c = r.calls();
    assert!(out.is_ok(), "{out:?} {c:?}");
    assert_eq!(
        c.iter().filter(|x| x.starts_with("build:")).count(),
        1,
        "{c:?}"
    );
}

#[tokio::test]
async fn g1_a_backoff_for_the_tag_defers_with_no_reuse_and_no_build() {
    let d = tempfile::tempdir().unwrap();
    record_attempt(d.path(), &tag(), NOW).unwrap();
    let r = unstamped_with_alias(d.path());
    let out = run_node_sequence(&r, &r.dir, &key_fp(), &pin(), || NOW + 1, true).await;
    let c = r.calls();
    assert_eq!(out.ok(), Some(Outcome::Ran), "{c:?}");
    assert!(
        !c.iter().any(|x| x.starts_with("build:") || x == "probe"),
        "{c:?}"
    );
    assert!(read_stamp(d.path()).is_none());
}

#[tokio::test]
async fn g1_a_probe_reject_writes_no_stamp_and_falls_back_as_install_does() {
    let d = tempfile::tempdir().unwrap();
    let mut r = unstamped_with_alias(d.path());
    r.probe_ok = false;
    let _ = seq(&r).await;
    let c = r.calls();
    assert!(c.iter().any(|x| x == "probe"), "{c:?}");
    // The reuse probe failed, so the build path ran exactly as before.
    assert_eq!(
        c.iter().filter(|x| x.starts_with("build:")).count(),
        1,
        "{c:?}"
    );
    assert!(
        !c.iter().any(|x| x == &format!("rmi:{OLD}")),
        "latest is never removed: {c:?}"
    );
}

#[tokio::test]
async fn g1_a_stop_during_the_reuse_proof_returns_stopped() {
    let d = tempfile::tempdir().unwrap();
    let mut r = unstamped_with_alias(d.path());
    r.stop_on_probe = true;
    let out = seq(&r).await;
    assert_eq!(out.ok(), Some(Outcome::Stopped), "{:?}", r.calls());
    assert!(read_stamp(d.path()).is_none());
}
