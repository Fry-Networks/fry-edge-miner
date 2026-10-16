//! v0.4.38 follow-up tests (RC38): K1 stop-mid-probe cleanup and its siblings.
//! The fake models a real container lifecycle: `compose stop` leaves the
//! container (stopped), only `down` removes it, and `rmi` of an image a
//! container still uses conflicts, as Docker does. Synthetic values only.
use super::super::diiisco_rebuild::{
    discard_candidate_by_id, drop_previous, fp12, gate_candidate, image_tag, ledger_add,
    ledger_read, ledger_remove, read_backoff, read_stamp, record_attempt, reject_candidate,
    rmi_log_text, rollback_attempt, rollback_latest, run_node_sequence, Gate, ImageLookup,
    ImageRef, NodeOps, Outcome, BACKOFF_FILE, LATEST_REF, STAMP_FILE,
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
    /// References whose lookup FAILS (docker error), as opposed to absent.
    error_refs: Vec<String>,
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
            error_refs: Vec::new(),
        }
    }

    /// One `docker rmi <id>`: false when a container still uses the image or
    /// the id is in `rmi_fail_ids`.
    fn try_rmi(&self, id: &str) -> bool {
        if self.container.lock().unwrap().as_deref() == Some(id) {
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
        if self.error_refs.iter().any(|r| r == reference) {
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

/// The K1 contract after a stop: the candidate container is removed (`down`)
/// AFTER the probe/up that it ran in and BEFORE its image is removed; the
/// image removal never conflicts; nothing is brought back up afterwards.
fn assert_k1_cleanup(r: &R, after: &str) {
    let c = r.calls();
    assert!(!r.has_prefix("rmi_conflict"), "rmi hit a container: {c:?}");
    let a = c
        .iter()
        .rposition(|x| x.starts_with(after))
        .unwrap_or(usize::MAX);
    let d = last(&c, "down");
    let m = at(&c, &format!("rmi:{NEW}"));
    assert!(
        a < d && d < m,
        "down must follow {after} and precede rmi: {c:?}"
    );
    assert!(
        !c[a + 1..]
            .iter()
            .any(|x| x.starts_with("up_") || x.starts_with("tag_latest")),
        "nothing may be brought up or promoted after the stop: {c:?}"
    );
}

#[tokio::test]
async fn k1_gate_stop_mid_probe_downs_before_the_image_is_removed() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.cand_exists.store(true, Ordering::SeqCst);
    r.stop_on_probe = true;
    r.probe_ok = false; // the real probe returns false when it sees a stop
    assert_eq!(gate_candidate(&r, &tag(), true).await, Gate::Stopped);
    assert_k1_cleanup(&r, "probe");
}

#[tokio::test]
async fn k1_gate_stop_after_a_passing_probe_downs_before_the_image_is_removed() {
    for have_image in [true, false] {
        let d = tempfile::tempdir().unwrap();
        let mut r = R::new(d.path(), have_image);
        r.cand_exists.store(true, Ordering::SeqCst);
        r.stop_on_probe = true;
        assert_eq!(gate_candidate(&r, &tag(), have_image).await, Gate::Stopped);
        assert_k1_cleanup(&r, "probe");
    }
}

#[tokio::test]
async fn k1_gate_stop_during_a_failed_up_is_stopped_and_downs_first() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.cand_exists.store(true, Ordering::SeqCst);
    r.stop_on_up = true;
    r.up_ok = false;
    assert_eq!(gate_candidate(&r, &tag(), true).await, Gate::Stopped);
    assert_k1_cleanup(&r, &format!("up_no_build:{}", tag()));
}

#[tokio::test]
async fn k1_reject_candidate_with_a_stop_pending_downs_first_and_brings_nothing_up() {
    let d = tempfile::tempdir().unwrap();
    let r = R::new(d.path(), true);
    r.cand_exists.store(true, Ordering::SeqCst);
    *r.container.lock().unwrap() = Some(NEW.into());
    r.stop.store(true, Ordering::SeqCst);
    reject_candidate(&r, &tag(), true);
    let c = r.calls();
    assert!(!r.has_prefix("rmi_conflict"), "{c:?}");
    assert!(at(&c, "down") < at(&c, &format!("rmi:{NEW}")), "{c:?}");
    assert!(!c.iter().any(|x| x.starts_with("up_")), "{c:?}");
}

#[tokio::test]
async fn k1_upgrade_stop_mid_probe_downs_first_and_never_touches_latest() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.stop_on_probe = true;
    r.probe_ok = false;
    assert_eq!(seq(&r).await.ok(), Some(Outcome::Stopped));
    assert_k1_cleanup(&r, "probe");
    let c = r.calls();
    assert!(!c.iter().any(|x| x == &format!("rmi:{OLD}")), "{c:?}");
    assert!(read_stamp(d.path()).is_none());
}

#[tokio::test]
async fn k1_upgrade_stop_after_a_passing_probe_downs_before_discard() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.stop_on_probe = true;
    assert_eq!(seq(&r).await.ok(), Some(Outcome::Stopped));
    assert_k1_cleanup(&r, "probe");
    assert!(read_stamp(d.path()).is_none());
}

#[tokio::test]
async fn k1_fresh_stop_mid_probe_is_stopped_with_no_backoff_and_downs_first() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), false);
    r.stop_on_probe = true;
    r.probe_ok = false;
    assert_eq!(seq(&r).await.ok(), Some(Outcome::Stopped));
    assert_k1_cleanup(&r, "probe");
    assert!(
        read_backoff(d.path()).is_none(),
        "a stop is not a failed attempt"
    );
    assert!(read_stamp(d.path()).is_none());
}

#[tokio::test]
async fn k1_fresh_failed_promotion_downs_and_discards_the_running_candidate() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), false);
    r.tag_ok = false;
    assert!(seq(&r).await.is_err());
    let c = r.calls();
    assert!(!r.has_prefix("rmi_conflict"), "{c:?}");
    assert!(
        last(&c, "down") > at(&c, &format!("tag_latest:{}", tag())),
        "the candidate container must be downed after the failed promotion: {c:?}"
    );
    assert!(c.contains(&format!("rmi:{NEW}")), "{c:?}");
    assert!(read_stamp(d.path()).is_none());
    assert_eq!(read_backoff(d.path()).map(|b| b.attempts), Some(1));
}

// ------------------------------------------------------------------- m6

fn cand_ref() -> String {
    format!("diiisco-node:{}", tag())
}

#[test]
fn m6_a_failed_latest_lookup_keeps_the_candidate() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.cand_exists.store(true, Ordering::SeqCst);
    r.error_refs = vec![LATEST_REF.to_string()];
    discard_candidate_by_id(&r, &tag());
    let c = r.calls();
    assert!(
        !r.has_prefix("rmi"),
        "an unknown latest must keep the image: {c:?}"
    );
}

#[test]
fn m6_a_failed_candidate_lookup_keeps_everything() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.cand_exists.store(true, Ordering::SeqCst);
    r.error_refs = vec![cand_ref()];
    discard_candidate_by_id(&r, &tag());
    assert!(!r.has_prefix("rmi"), "{:?}", r.calls());
}

#[test]
fn m6_an_absent_latest_still_lets_the_candidate_go() {
    // control for the two tests above: absent is not an error
    let d = tempfile::tempdir().unwrap();
    let r = R::new(d.path(), false);
    r.cand_exists.store(true, Ordering::SeqCst);
    discard_candidate_by_id(&r, &tag());
    assert!(r.calls().contains(&format!("rmi:{NEW}")), "{:?}", r.calls());
}

#[test]
fn m6_a_failed_new_image_lookup_never_removes_the_previous_image() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.cand_exists.store(true, Ordering::SeqCst);
    r.error_refs = vec![cand_ref()];
    *r.container.lock().unwrap() = None; // the previous node is already replaced
    drop_previous(&r, Some(OLD), &tag());
    assert!(!r.has_prefix("rmi"), "{:?}", r.calls());
}

#[test]
fn m6_drop_previous_with_a_known_different_new_image_removes_the_previous() {
    // control: the lookup succeeds, so the previous image goes
    let d = tempfile::tempdir().unwrap();
    let r = R::new(d.path(), true);
    r.cand_exists.store(true, Ordering::SeqCst);
    *r.container.lock().unwrap() = None; // the previous node is already replaced
    drop_previous(&r, Some(OLD), &tag());
    assert!(r.calls().contains(&format!("rmi:{OLD}")), "{:?}", r.calls());
}

#[tokio::test]
async fn m6_an_upgrade_whose_new_image_lookup_fails_keeps_the_previous_image() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.error_refs = vec![cand_ref()];
    // the promotion itself succeeds; only the post-promotion lookup fails
    assert_eq!(seq(&r).await.ok(), Some(Outcome::Upgraded));
    assert!(
        !r.calls().contains(&format!("rmi:{OLD}")),
        "{:?}",
        r.calls()
    );
}

// ------------------------------------------------------------------- m4

/// A stamp that cannot be written: the stamp path is a directory.
fn break_stamp(dir: &Path) {
    std::fs::create_dir(dir.join(STAMP_FILE)).unwrap();
}

#[tokio::test]
async fn m4_an_upgrade_whose_stamp_cannot_be_written_rolls_latest_back() {
    let d = tempfile::tempdir().unwrap();
    break_stamp(d.path());
    let r = R::new(d.path(), true);
    let out = seq(&r).await.ok();
    let c = r.calls();
    assert_eq!(out, Some(Outcome::UpgradeFailed), "{c:?}");
    assert!(
        at(&c, &format!("retag_latest:{OLD}")) < at(&c, "up_no_build:none"),
        "latest must point at the old image before the old node is restored: {c:?}"
    );
    assert!(c.contains(&format!("rmi:{NEW}")), "{c:?}");
    assert!(!c.contains(&format!("rmi:{OLD}")), "{c:?}");
    assert_eq!(read_backoff(d.path()).map(|b| b.attempts), Some(1));
}

#[tokio::test]
async fn m4_a_fresh_promotion_whose_stamp_fails_is_cleaned_up_and_reported() {
    // no previous image: the proven candidate stays latest, the node is down
    let d = tempfile::tempdir().unwrap();
    break_stamp(d.path());
    let r = R::new(d.path(), false);
    assert!(seq(&r).await.is_err());
    let c = r.calls();
    assert!(!r.has_prefix("rmi_conflict"), "{c:?}");
    assert!(
        last(&c, "down") > at(&c, &format!("tag_latest:{}", tag())),
        "{c:?}"
    );
    assert!(!c.iter().any(|x| x.starts_with("retag_latest")), "{c:?}");
    assert_eq!(read_backoff(d.path()).map(|b| b.attempts), Some(1));
}

#[test]
fn m4_rollback_latest_points_latest_back_at_the_previous_image() {
    let d = tempfile::tempdir().unwrap();
    let r = R::new(d.path(), true);
    rollback_latest(&r, Some(OLD));
    assert_eq!(r.calls(), vec![format!("retag_latest:{OLD}")]);
    let r = R::new(d.path(), false);
    rollback_latest(&r, None);
    assert!(
        r.calls().is_empty(),
        "no previous image, nothing to restore"
    );
}

#[test]
fn m4_install_rolls_latest_back_before_rejecting_a_candidate_whose_stamp_failed() {
    let src = include_str!("diiisco.rs");
    let body = src.split("#[cfg(test)]").next().unwrap();
    let install = body.split("async fn install(&self)").nth(1).unwrap();
    let install = install.split("async fn start(").next().unwrap();
    let a = install.find("after_fresh_build(").unwrap();
    let r = install.find("rollback_latest(").unwrap_or(usize::MAX);
    let j = install.find("reject_candidate(").unwrap();
    assert!(
        a < r && r < j,
        "after_fresh_build {a}, rollback_latest {r}, reject {j}"
    );
}

// ------------------------------------------------------------------- m5

#[test]
fn m5_rollback_attempt_restores_the_prior_record_or_clears_it() {
    let d = tempfile::tempdir().unwrap();
    record_attempt(d.path(), &tag(), 10).unwrap();
    rollback_attempt(d.path(), None);
    assert!(read_backoff(d.path()).is_none(), "no prior record: cleared");

    record_attempt(d.path(), &tag(), 10).unwrap();
    let prior = read_backoff(d.path());
    record_attempt(d.path(), &tag(), 99).unwrap();
    assert_eq!(read_backoff(d.path()).map(|b| b.attempts), Some(2));
    rollback_attempt(d.path(), prior.clone());
    assert_eq!(read_backoff(d.path()), prior, "the prior record is back");
}

#[test]
fn m5_install_reports_a_stopped_gate_as_an_error_and_undoes_its_attempt() {
    let src = include_str!("diiisco.rs");
    let body = src.split("#[cfg(test)]").next().unwrap();
    let install = body.split("async fn install(&self)").nth(1).unwrap();
    let install = install.split("async fn start(").next().unwrap();
    assert!(
        !install.contains("Gate::Stopped => return Ok(())"),
        "a stopped install must not look installed"
    );
    let arm = install
        .split("Gate::Stopped =>")
        .nth(1)
        .unwrap_or("")
        .split("// `latest`")
        .next()
        .unwrap_or("");
    assert!(arm.contains("rollback_attempt("), "arm: {arm}");
    assert!(arm.contains("bail!("), "arm: {arm}");
    let prior = install.find("read_backoff(").unwrap_or(usize::MAX);
    let record = install.find("record_attempt(").unwrap();
    assert!(
        prior < record,
        "the prior record is read before the new one"
    );
}

// ------------------------------------------------------------------- G6

#[test]
fn g6_the_upgrade_records_its_attempt_after_the_build_like_install() {
    let seq_src = include_str!("diiisco_rebuild.rs");
    let body = seq_src.split("#[cfg(test)]").next().unwrap();
    let upgrade = body.split("// Upgrade:").nth(1).unwrap();
    let rec = upgrade.find("record_attempt(dir").unwrap_or(usize::MAX);
    let build = upgrade.find("ops.build(").unwrap();
    assert!(build < rec, "build at {build}, first record at {rec}");
}

#[tokio::test]
async fn g6_a_stop_during_the_upgrade_build_leaves_no_backoff() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.stop_on_build = true;
    assert_eq!(seq(&r).await.ok(), Some(Outcome::Stopped));
    assert!(
        read_backoff(d.path()).is_none(),
        "an interrupted build is not a failure"
    );
}

#[tokio::test]
async fn g6_a_failed_upgrade_build_still_records_an_attempt() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.build_ok = false;
    assert_eq!(seq(&r).await.ok(), Some(Outcome::UpgradeFailed));
    assert_eq!(read_backoff(d.path()).map(|b| b.attempts), Some(1));
}

#[tokio::test]
async fn g6_a_stop_in_the_gate_restores_the_backoff_it_found() {
    let d = tempfile::tempdir().unwrap();
    // an old, expired attempt for this target
    let old = std::fs::write(
        d.path().join(BACKOFF_FILE),
        format!("attempts=1\nlast=1000000\ntag={}\n", tag()),
    );
    old.unwrap();
    let prior = read_backoff(d.path());
    assert!(prior.is_some());
    let mut r = R::new(d.path(), true);
    r.stop_on_probe = true;
    r.probe_ok = false;
    assert_eq!(seq(&r).await.ok(), Some(Outcome::Stopped));
    assert_eq!(
        read_backoff(d.path()),
        prior,
        "a stop is not a failed attempt"
    );
}

#[tokio::test]
async fn g6_a_promoted_upgrade_clears_the_backoff() {
    let d = tempfile::tempdir().unwrap();
    let r = R::new(d.path(), true);
    assert_eq!(seq(&r).await.ok(), Some(Outcome::Upgraded));
    assert!(read_backoff(d.path()).is_none());
}

// ------------------------------------------------------------------- G7

#[test]
fn g7_the_removal_log_line_names_what_was_removed() {
    for what in [
        "previous node image",
        "failed candidate image",
        "stale candidate image",
    ] {
        assert!(rmi_log_text(what, true).contains(what), "{what}");
        assert!(rmi_log_text(what, false).contains(what), "{what}");
        assert!(rmi_log_text(what, false).contains("not removed"));
    }
    assert!(!rmi_log_text("failed candidate image", true).contains("previous"));
}

#[test]
fn g7_a_failed_candidate_is_logged_as_a_failed_candidate() {
    let d = tempfile::tempdir().unwrap();
    let r = R::new(d.path(), false);
    r.cand_exists.store(true, Ordering::SeqCst);
    discard_candidate_by_id(&r, &tag());
    assert!(
        r.calls()
            .contains(&"rmi_label:failed candidate image".to_string()),
        "{:?}",
        r.calls()
    );
}

#[test]
fn g7_the_previous_image_is_logged_as_the_previous_image() {
    let d = tempfile::tempdir().unwrap();
    let r = R::new(d.path(), true);
    r.cand_exists.store(true, Ordering::SeqCst);
    *r.container.lock().unwrap() = None;
    drop_previous(&r, Some(OLD), &tag());
    assert!(
        r.calls()
            .contains(&"rmi_label:previous node image".to_string()),
        "{:?}",
        r.calls()
    );
}

#[tokio::test]
async fn g7_the_upgrade_logs_its_previous_image_removal_by_name() {
    let d = tempfile::tempdir().unwrap();
    let r = R::new(d.path(), true);
    assert_eq!(seq(&r).await.ok(), Some(Outcome::Upgraded));
    assert!(
        r.calls()
            .contains(&"rmi_label:previous node image".to_string()),
        "{:?}",
        r.calls()
    );
}

#[test]
fn g7_docker_node_has_no_fixed_previous_image_wording() {
    let src = include_str!("diiisco.rs");
    let body = src.split("#[cfg(test)]").next().unwrap();
    assert!(!body.contains("removed the previous node image"));
    assert!(!body.contains("previous node image not removed"));
}

// ------------------------------------------------------------------- G8

#[test]
fn g8_a_failed_candidate_that_took_the_stamp_tag_gives_the_alias_back() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.cand_exists.store(true, Ordering::SeqCst);
    r.stamp_tag = Some(tag());
    *r.container.lock().unwrap() = Some(OLD.into());
    discard_candidate_by_id(&r, &tag());
    let c = r.calls();
    assert!(
        c.contains(&format!("retag_alias:{}:{OLD}", tag())),
        "the alias must be re-pointed at latest: {c:?}"
    );
    assert!(
        at(&c, &format!("rmi:{NEW}")) < at(&c, &format!("retag_alias:{}:{OLD}", tag())),
        "latest keeps its fem- alias: {c:?}"
    );
}

#[test]
fn g8_no_alias_is_touched_when_the_stamp_tag_is_another_tag() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.cand_exists.store(true, Ordering::SeqCst);
    r.stamp_tag = Some("fem-000000000000".into());
    *r.container.lock().unwrap() = Some(OLD.into());
    discard_candidate_by_id(&r, &tag());
    let c = r.calls();
    assert!(
        c.contains(&format!("rmi:{NEW}")),
        "control: the candidate went: {c:?}"
    );
    assert!(!r.has_prefix("retag_alias"), "{c:?}");
}

#[test]
fn g8_nothing_is_retagged_when_the_candidate_is_kept() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.cand_exists.store(true, Ordering::SeqCst);
    r.stamp_tag = Some(tag());
    r.error_refs = vec![LATEST_REF.to_string()];
    discard_candidate_by_id(&r, &tag());
    assert!(
        !r.has_prefix("rmi") && !r.has_prefix("retag_alias"),
        "{:?}",
        r.calls()
    );
}

// ----------------------------------------------------- G2: the promotion sweep

const STALE: &str = "sha256:a03f2386ae06b21109577020844df367857b72c2fcce384c1896fed98a89c82b";
const LEG: &str = "sha256:c49fea7425fa7f8699897a97c159c6690267d9003bb78c53fafa8fc15c325d84";

fn swept(r: &R, id: &str) -> bool {
    r.calls().contains(&format!("rmi:{id}"))
}

fn no_sweep(r: &R) -> bool {
    !r.calls()
        .iter()
        .any(|c| c == "rmi_label:stale candidate image")
}

fn stale_world(r: &mut R, dir: &Path) {
    ledger_add(dir, STALE).unwrap();
    r.images = Some(vec![
        ImageRef {
            id: LEG.into(),
            tag: Some("fem-0123456789ab".into()),
        },
        ImageRef {
            id: "sha256:656771905e1ef731f65cd0a0d9fb061238380a1a012e6abdf846ecc7d2ea36fd".into(),
            tag: Some("v1".into()),
        },
    ]);
    r.fem_ids = vec![LEG.into()];
}

#[tokio::test]
async fn g2_a_promoted_upgrade_sweeps_stale_candidates_and_nothing_else() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    stale_world(&mut r, d.path());
    assert_eq!(seq(&r).await.ok(), Some(Outcome::Upgraded));
    let c = r.calls();
    assert!(swept(&r, STALE) && swept(&r, LEG), "{c:?}");
    assert!(
        !swept(
            &r,
            "sha256:656771905e1ef731f65cd0a0d9fb061238380a1a012e6abdf846ecc7d2ea36fd"
        ),
        "{c:?}"
    );
    assert!(!swept(&r, NEW), "the promoted image stays: {c:?}");
    assert!(
        c.contains(&"rmi_label:stale candidate image".to_string()),
        "{c:?}"
    );
    assert!(
        at(&c, &format!("tag_latest:{}", tag())) < at(&c, &format!("rmi:{STALE}")),
        "sweep only after the promotion: {c:?}"
    );
    assert!(
        ledger_read(d.path()).is_empty(),
        "the swept and promoted ids are off the ledger"
    );
}

#[tokio::test]
async fn g2_a_promoted_fresh_start_sweeps_stale_candidates() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), false);
    stale_world(&mut r, d.path());
    assert_eq!(seq(&r).await.ok(), Some(Outcome::BuiltFresh));
    let c = r.calls();
    assert!(swept(&r, STALE) && swept(&r, LEG), "{c:?}");
    assert!(
        !swept(&r, NEW)
            && !swept(
                &r,
                "sha256:656771905e1ef731f65cd0a0d9fb061238380a1a012e6abdf846ecc7d2ea36fd"
            ),
        "{c:?}"
    );
}

#[tokio::test]
async fn g2_no_sweep_after_a_failed_promotion() {
    // upgrade: probe rejects / tag fails / stamp fails; fresh: probe rejects
    type Setup = fn(&mut R, &Path);
    let cases: [(&str, bool, Setup); 4] = [
        ("upgrade probe", true, |r, _| r.probe_ok = false),
        ("upgrade tag", true, |r, _| r.tag_ok = false),
        ("upgrade stamp", true, |_, d| break_stamp(d)),
        ("fresh probe", false, |r, _| r.probe_ok = false),
    ];
    for (name, have_image, set) in cases {
        let d = tempfile::tempdir().unwrap();
        let mut r = R::new(d.path(), have_image);
        stale_world(&mut r, d.path());
        set(&mut r, d.path());
        let _ = seq(&r).await;
        assert!(
            !swept(&r, STALE) && !swept(&r, LEG),
            "{name}: {:?}",
            r.calls()
        );
        assert!(no_sweep(&r), "{name}: {:?}", r.calls());
    }
}

#[tokio::test]
async fn g2_no_sweep_after_a_stop() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    stale_world(&mut r, d.path());
    r.stop_on_probe = true;
    assert_eq!(seq(&r).await.ok(), Some(Outcome::Stopped));
    assert!(!swept(&r, STALE) && !swept(&r, LEG), "{:?}", r.calls());
}

#[tokio::test]
async fn g2_unknown_container_state_removes_nothing() {
    for what in ["running", "listing"] {
        let d = tempfile::tempdir().unwrap();
        let mut r = R::new(d.path(), true);
        stale_world(&mut r, d.path());
        // the promotion itself needs the running list for nothing; only the sweep reads it
        if what == "running" {
            r.running_unknown = true;
        } else {
            r.images = None;
        }
        assert_eq!(seq(&r).await.ok(), Some(Outcome::Upgraded), "{what}");
        assert!(
            !swept(&r, STALE) && !swept(&r, LEG),
            "{what}: {:?}",
            r.calls()
        );
    }
}

#[tokio::test]
async fn g2_the_candidate_is_recorded_in_the_ledger_right_after_its_build() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.probe_ok = false;
    let _ = seq(&r).await;
    let c = r.calls();
    let b = at(&c, &format!("build:{}", tag()));
    assert_eq!(
        at(&c, &format!("record_candidate:{}", tag())),
        b + 1,
        "the ledger entry follows the build immediately: {c:?}"
    );
}

#[test]
fn g2_install_records_the_candidate_and_sweeps_after_the_previous_image_is_dropped() {
    let src = include_str!("diiisco.rs");
    let body = src.split("#[cfg(test)]").next().unwrap();
    let install = body.split("async fn install(&self)").nth(1).unwrap();
    let install = install.split("async fn start(").next().unwrap();
    let at = |n: &str| install.find(n).unwrap_or(usize::MAX);
    assert!(
        at("\"compose\", \"build\"") < at("record_candidate("),
        "ledger after the build"
    );
    assert!(
        at("record_candidate(") < at("gate_candidate("),
        "ledger before the gate"
    );
    assert!(
        at("drop_previous(") < at("sweep_stale_candidates("),
        "sweep after drop_previous"
    );
    assert_ne!(at("sweep_stale_candidates("), usize::MAX);
}

#[test]
fn g2_the_sweep_adds_no_second_rmi_site() {
    let src = include_str!("diiisco.rs");
    let body = src.split("#[cfg(test)]").next().unwrap();
    assert_eq!(body.matches("\"rmi\"").count(), 1);
    let reb = include_str!("diiisco_rebuild.rs");
    let reb = reb.split("#[cfg(test)]").next().unwrap();
    assert!(
        !reb.contains("\"rmi\""),
        "the rebuild module never builds a docker rmi argv"
    );
    assert!(!reb.contains("\"prune\""), "no prune argv anywhere");
}

// ------------------------------------------------- R1: bookkeeping needs a real rmi

#[test]
fn r1_a_failed_candidate_rmi_keeps_the_ledger_entry_and_does_not_move_the_alias() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.cand_exists.store(true, Ordering::SeqCst);
    r.stamp_tag = Some(tag());
    *r.container.lock().unwrap() = Some(OLD.into());
    r.rmi_fail_ids = vec![NEW.to_string()];
    ledger_add(d.path(), NEW).unwrap();
    discard_candidate_by_id(&r, &tag());
    let c = r.calls();
    assert!(
        c.contains(&format!("rmi_failed:{NEW}")),
        "control: the rmi was tried and failed: {c:?}"
    );
    assert_eq!(
        ledger_read(d.path()),
        vec![NEW.to_string()],
        "the image still exists: keep its entry"
    );
    assert!(
        !r.has_prefix("retag_alias"),
        "fem-X must stay on the image that still has it: {c:?}"
    );
}

#[test]
fn r1_a_successful_candidate_rmi_still_forgets_and_retags() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    r.cand_exists.store(true, Ordering::SeqCst);
    r.stamp_tag = Some(tag());
    *r.container.lock().unwrap() = Some(OLD.into());
    ledger_add(d.path(), NEW).unwrap();
    discard_candidate_by_id(&r, &tag());
    assert!(ledger_read(d.path()).is_empty());
    assert!(r.calls().contains(&format!("retag_alias:{}:{OLD}", tag())));
}

#[tokio::test]
async fn r1_a_failed_sweep_rmi_keeps_the_ledger_entry() {
    let d = tempfile::tempdir().unwrap();
    let mut r = R::new(d.path(), true);
    stale_world(&mut r, d.path());
    r.rmi_fail_ids = vec![STALE.to_string()];
    assert_eq!(seq(&r).await.ok(), Some(Outcome::Upgraded));
    let c = r.calls();
    assert!(
        c.contains(&format!("rmi_failed:{STALE}")),
        "control: tried and failed: {c:?}"
    );
    assert!(ledger_read(d.path()).contains(&STALE.to_string()), "{c:?}");
    assert!(swept(&r, LEG), "control: the other stale image went: {c:?}");
}
