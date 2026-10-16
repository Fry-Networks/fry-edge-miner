//! v0.4.38 follow-up tests (RC38): K1 stop-mid-probe cleanup and its siblings.
//! The fake models a real container lifecycle: `compose stop` leaves the
//! container (stopped), only `down` removes it, and `rmi` of an image a
//! container still uses conflicts, as Docker does. Synthetic values only.
use super::super::diiisco_rebuild::{
    discard_candidate_by_id, drop_previous, fp12, gate_candidate, image_tag, read_backoff,
    read_stamp, reject_candidate, run_node_sequence, Gate, ImageLookup, NodeOps, Outcome,
    LATEST_REF,
};
use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const NOW: u64 = 2_000_000;
const OLD: &str = "sha256:old";
const NEW: &str = "sha256:new";

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
            error_refs: Vec::new(),
        }
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
    fn rmi_by_id(&self, id: &str) {
        if self.container.lock().unwrap().as_deref() == Some(id) {
            self.rec(format!("rmi_conflict:{id}"));
            return;
        }
        self.rec(format!("rmi:{id}"));
        if id == NEW {
            self.cand_exists.store(false, Ordering::SeqCst);
        }
    }
    fn discard_candidate(&self, t: &str) {
        super::super::diiisco_rebuild::discard_candidate_by_id(self, t);
    }
    /// `compose stop` leaves a stopped container: not running, still present.
    fn running_image_ids(&self) -> Option<Vec<String>> {
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
