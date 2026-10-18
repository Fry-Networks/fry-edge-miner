//! A4 polish items on top of the RC38 fake (a copy: that module's fake is
//! private). Synthetic values only; the Docker side is a recording fake.
#![allow(dead_code)]
use super::super::diiisco_rebuild::{
    discard_candidate_by_id, fp12, image_tag, kept_reason, ledger_add, ledger_remove,
    note_install_reject, rmi_outcome, run_node_sequence, ImageLookup, ImageRef, NodeOps, Outcome,
    RmiOutcome, LATEST_REF,
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
    fn retag_alias(&self, t: &str, id: &str) -> Result<()> {
        self.rec(format!("retag_alias:{t}:{id}"));
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

#[tokio::test]
async fn f3_an_install_reject_of_a_fresh_node_makes_start_back_off_too() {
    let d = tempfile::tempdir().unwrap();
    note_install_reject(d.path(), &tag(), NOW, false, true);
    let r = R::new(d.path(), false);
    let err = run_node_sequence(&r, &r.dir, &key_fp(), &pin(), || NOW + 10, true)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("backing off"), "{err}");
    assert!(!r.has_prefix("build:"), "{:?}", r.calls());
}

#[tokio::test]
async fn f3_only_a_key_rejection_of_a_fresh_install_arms_the_cooldown() {
    // A node image already exists: the upgrade path has its own backoff.
    let d = tempfile::tempdir().unwrap();
    note_install_reject(d.path(), &tag(), NOW, true, true);
    // Docker trouble (the probe never ran) is not a verdict on the key.
    note_install_reject(d.path(), &tag(), NOW, false, false);
    let r = R::new(d.path(), false);
    let out = run_node_sequence(&r, &r.dir, &key_fp(), &pin(), || NOW + 10, true).await;
    assert_eq!(out.ok(), Some(Outcome::BuiltFresh), "{:?}", r.calls());
}

// ------------------------------------------------------------------------ L7

#[test]
fn l7_a_candidate_that_is_the_current_image_is_named_as_such() {
    let current = kept_reason(Some(NEW), Some(NEW));
    assert!(current.contains("current image"), "{current}");
    // Any other reason does not claim it.
    let other = kept_reason(Some(NEW), Some(OLD));
    assert!(!other.contains("current image"), "{other}");
    assert!(other.contains("in use"), "{other}");
    assert!(kept_reason(Some(NEW), None).contains("in use"));
}

// ------------------------------------------------------------- No such image

#[test]
fn an_rmi_of_an_image_docker_no_longer_has_is_already_gone_not_a_failure() {
    let gone = "Error response from daemon: No such image: sha256:0123abcd\n";
    assert_eq!(rmi_outcome(false, gone), RmiOutcome::AlreadyGone);
    assert_eq!(rmi_outcome(true, ""), RmiOutcome::Removed);
}

#[test]
fn every_other_rmi_failure_stays_a_failure() {
    for e in [
        "Error response from daemon: conflict: unable to delete sha256:0123 (cannot be forced) - image is being used by running container abc",
        "Error response from daemon: Get \"http://docker\": connection refused",
        "",
    ] {
        assert_eq!(rmi_outcome(false, e), RmiOutcome::Failed, "{e}");
    }
    // A success is never reclassified by stray stderr text.
    assert_eq!(rmi_outcome(true, "No such image"), RmiOutcome::Removed);
}

// ------------------------------------------------------------------------ m4

#[test]
fn m4_an_unreadable_stamp_still_gives_latest_its_alias_back() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.cand_exists.store(true, Ordering::SeqCst);
    r.stamp_unreadable = true;
    discard_candidate_by_id(&r, &tag());
    let c = r.calls();
    assert!(
        c.contains(&format!("retag_alias:{}:{OLD}", tag())),
        "alias restored for latest: {c:?}"
    );
}

#[test]
fn m4_no_stamp_at_all_or_another_tag_touches_no_alias() {
    // Never stamped: nothing says latest ever carried this tag.
    let d = tempfile::tempdir().unwrap();
    let r = R::new(d.path(), true);
    r.cand_exists.store(true, Ordering::SeqCst);
    discard_candidate_by_id(&r, &tag());
    assert!(!r.has_prefix("retag_alias"), "{:?}", r.calls());

    // A readable stamp for another tag.
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.cand_exists.store(true, Ordering::SeqCst);
    r.stamp_tag = Some("fem-000000000000".into());
    discard_candidate_by_id(&r, &tag());
    assert!(!r.has_prefix("retag_alias"), "{:?}", r.calls());
}

#[test]
fn m4_an_unreadable_stamp_with_no_latest_has_nothing_to_restore() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), false);
    r.cand_exists.store(true, Ordering::SeqCst);
    r.stamp_unreadable = true;
    discard_candidate_by_id(&r, &tag());
    assert!(!r.has_prefix("retag_alias"), "{:?}", r.calls());
}

// ------------------------------------------------------------------------ R3

fn break_stamp(dir: &Path) {
    std::fs::create_dir(dir.join(super::super::diiisco_rebuild::STAMP_FILE)).unwrap();
}

#[tokio::test]
async fn r3_a_transient_lookup_error_before_the_build_does_not_block_the_rollback() {
    let d = tempfile::tempdir().unwrap();
    break_stamp(d.path());
    let r = R::new(d.path(), true);
    *r.error_once.lock().unwrap() = vec![LATEST_REF.to_string()];
    let out = run_node_sequence(&r, &r.dir, &key_fp(), &pin(), || NOW, true)
        .await
        .ok();
    let c = r.calls();
    assert_eq!(out, Some(Outcome::UpgradeFailed), "{c:?}");
    assert!(
        c.contains(&format!("retag_latest:{OLD}")),
        "latest pointed back at the previous image: {c:?}"
    );
}

#[tokio::test]
async fn r3_a_lookup_that_keeps_failing_still_keeps_everything() {
    let d = tempfile::tempdir().unwrap();
    break_stamp(d.path());
    let mut r = R::new(d.path(), true);
    r.error_refs = vec![LATEST_REF.to_string()];
    let out = run_node_sequence(&r, &r.dir, &key_fp(), &pin(), || NOW, true)
        .await
        .ok();
    let c = r.calls();
    assert_eq!(out, Some(Outcome::UpgradeFailed), "{c:?}");
    assert!(!c.iter().any(|x| x.starts_with("retag_latest")), "{c:?}");
    assert!(!c.iter().any(|x| x.starts_with("rmi")), "{c:?}");
}

// ------------------------------------------------------------------------ A5

#[tokio::test]
async fn a5_a_fresh_reject_and_the_cooldown_that_follows_it_are_typed_rejections() {
    use crate::integrations::diiisco::NodeRejected;
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), false);
    r.probe_ok = false;
    let err = run_node_sequence(&r, &r.dir, &key_fp(), &pin(), || NOW, true)
        .await
        .unwrap_err();
    assert!(err.downcast_ref::<NodeRejected>().is_some(), "{err:#}");
    assert!(err.to_string().contains("nothing was promoted"), "{err}");

    let err = run_node_sequence(&r, &r.dir, &key_fp(), &pin(), || NOW + 10, true)
        .await
        .unwrap_err();
    assert!(err.downcast_ref::<NodeRejected>().is_some(), "{err:#}");
    assert!(err.to_string().contains("backing off"), "{err}");
}

#[tokio::test]
async fn a5_docker_trouble_and_a_missing_pin_are_not_rejections() {
    use crate::integrations::diiisco::NodeRejected;
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), false);
    r.build_ok = false;
    let err = run_node_sequence(&r, &r.dir, &key_fp(), &pin(), || NOW, true)
        .await
        .unwrap_err();
    assert!(err.downcast_ref::<NodeRejected>().is_none(), "{err:#}");
}
