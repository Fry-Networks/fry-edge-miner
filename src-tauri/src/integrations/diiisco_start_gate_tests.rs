//! start()'s fresh (missing-image) branch is gated on the F2 probe before it
//! promotes, exactly like install(). Synthetic values only; the Docker side is
//! a recording fake.
use super::super::diiisco_rebuild::{
    backoff_defers, discard_candidate_by_id, fp12, image_tag, read_backoff, read_stamp,
    record_attempt, run_node_sequence, stamp_is_current, NodeOps, Outcome, Stamp, LATEST_REF,
};
use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

const NOW: u64 = 3_000_000;
const LATEST_ID: &str = "sha256:old";
const CAND_ID: &str = "sha256:new";

fn key_fp() -> String {
    fp12(&format!("sk-fem-{}", "ab".repeat(32)))
}

fn pin() -> String {
    "a".repeat(40)
}

fn tag() -> String {
    image_tag(&key_fp(), &pin())
}

struct G {
    dir: PathBuf,
    calls: Mutex<Vec<String>>,
    builds: AtomicUsize,
    cand_exists: AtomicBool,
    up_ok: bool,
    probe_ok: bool,
    running: Option<Vec<String>>,
    stop: AtomicBool,
    stop_on_probe: bool,
    stamp_at_tag_latest: Mutex<Option<bool>>,
}

impl G {
    fn new(dir: &Path) -> Self {
        G {
            dir: dir.to_path_buf(),
            calls: Mutex::new(Vec::new()),
            builds: AtomicUsize::new(0),
            cand_exists: AtomicBool::new(false),
            up_ok: true,
            probe_ok: true,
            running: Some(Vec::new()),
            stop: AtomicBool::new(false),
            stop_on_probe: false,
            stamp_at_tag_latest: Mutex::new(None),
        }
    }

    fn rec(&self, c: String) {
        self.calls.lock().unwrap().push(c);
    }

    /// Every recorded call except the image-id lookups.
    fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| !c.starts_with("image_id:"))
            .cloned()
            .collect()
    }

    fn has(&self, what: &str) -> bool {
        self.calls().iter().any(|c| c == what)
    }

    fn rmis(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|c| c.starts_with("rmi:"))
            .collect()
    }
}

/// The image is Absent (`image_exists` false), yet an unrelated `latest` id is
/// reported so "never removes latest" is not vacuous.
#[async_trait]
impl NodeOps for G {
    fn image_exists(&self) -> bool {
        false
    }
    fn refresh_dockerfile(&self) -> Result<()> {
        self.rec("refresh_dockerfile".into());
        Ok(())
    }
    fn image_id(&self, reference: &str) -> Option<String> {
        self.rec(format!("image_id:{reference}"));
        if reference == LATEST_REF {
            return Some(LATEST_ID.into());
        }
        self.cand_exists
            .load(Ordering::SeqCst)
            .then(|| CAND_ID.into())
    }
    fn build(&self, t: &str) -> Result<()> {
        self.rec(format!("build:{t}"));
        self.builds.fetch_add(1, Ordering::SeqCst);
        self.cand_exists.store(true, Ordering::SeqCst);
        Ok(())
    }
    fn down(&self) {
        self.rec("down".into());
    }
    fn up_plain(&self) -> Result<()> {
        self.rec("up_plain".into());
        Ok(())
    }
    fn up_no_build(&self, t: Option<&str>) -> Result<()> {
        self.rec(format!("up_no_build:{}", t.unwrap_or("none")));
        if t.is_some() && !self.up_ok {
            anyhow::bail!("fake up failed");
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
        *self.stamp_at_tag_latest.lock().unwrap() =
            Some(stamp_is_current(&self.dir, &key_fp(), &pin()));
        Ok(())
    }
    fn rmi_by_id(&self, id: &str) {
        self.rec(format!("rmi:{id}"));
    }
    fn discard_candidate(&self, t: &str) {
        discard_candidate_by_id(self, t);
    }
    fn running_image_ids(&self) -> Option<Vec<String>> {
        self.running.clone()
    }
    fn stop_requested(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

async fn run(g: &G, rebuild_enabled: bool) -> Result<Outcome> {
    run_node_sequence(g, &g.dir, &key_fp(), &pin(), || NOW, rebuild_enabled).await
}

fn pos(c: &[String], what: &str) -> usize {
    c.iter()
        .position(|x| x == what)
        .unwrap_or_else(|| panic!("missing {what} in {c:?}"))
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn never_removed_latest(g: &G) {
    assert!(
        !g.rmis().contains(&format!("rmi:{LATEST_ID}")),
        "latest was removed: {:?}",
        g.calls()
    );
}

#[tokio::test]
async fn a_rejected_fresh_node_is_discarded_by_id_and_never_promoted() {
    let d = tempfile::tempdir().unwrap();
    let mut g = G::new(d.path());
    g.probe_ok = false;
    let err = run(&g, true).await.unwrap_err();
    assert!(err.to_string().contains("nothing was promoted"), "{err}");
    let t = tag();
    assert_eq!(
        g.calls()
            .into_iter()
            .filter(|c| !c.starts_with("rmi:"))
            .collect::<Vec<_>>(),
        vec![
            "refresh_dockerfile".to_string(),
            format!("build:{t}"),
            format!("up_no_build:{t}"),
            "probe".to_string(),
            "down".to_string(),
        ]
    );
    assert!(!g.has(&format!("tag_latest:{t}")));
    assert!(!g.has("up_plain"));
    assert_eq!(g.rmis(), s(&[&format!("rmi:{CAND_ID}")]));
    never_removed_latest(&g);
    assert!(read_stamp(d.path()).is_none());
    let b = read_backoff(d.path()).expect("attempt recorded");
    assert_eq!((b.tag.as_str(), b.attempts), (t.as_str(), 1));
}

#[tokio::test]
async fn a_fresh_up_failure_or_probe_outage_promotes_nothing_and_backs_off() {
    // (a) the candidate fails to start: no probe is ever made.
    let d = tempfile::tempdir().unwrap();
    let mut g = G::new(d.path());
    g.up_ok = false;
    assert!(run(&g, true).await.is_err());
    let t = tag();
    assert!(!g.has("probe"));
    assert!(!g.has(&format!("tag_latest:{t}")));
    assert!(!g.has("up_plain"));
    assert_eq!(g.rmis(), s(&[&format!("rmi:{CAND_ID}")]));
    never_removed_latest(&g);
    assert!(read_stamp(d.path()).is_none());
    assert_eq!(read_backoff(d.path()).map(|b| b.attempts), Some(1));

    // (b) the node never proves the key (probe outage): same seam as a reject.
    let d = tempfile::tempdir().unwrap();
    let mut g = G::new(d.path());
    g.probe_ok = false;
    assert!(run(&g, true).await.is_err());
    assert!(g.has("probe"));
    assert!(!g.has(&format!("tag_latest:{t}")));
    assert!(!g.has("up_plain"));
    assert_eq!(g.rmis(), s(&[&format!("rmi:{CAND_ID}")]));
    never_removed_latest(&g);
    assert!(read_stamp(d.path()).is_none());
    assert_eq!(read_backoff(d.path()).map(|b| b.attempts), Some(1));
}

#[tokio::test]
async fn a_passing_fresh_node_is_probed_before_it_is_promoted_and_stamped() {
    let d = tempfile::tempdir().unwrap();
    record_attempt(d.path(), &tag(), NOW - 100_000).unwrap();
    let g = G::new(d.path());
    assert_eq!(run(&g, true).await.unwrap(), Outcome::BuiltFresh);
    let t = tag();
    let c = g.calls();
    assert_eq!(g.builds.load(Ordering::SeqCst), 1);
    assert!(g.has(&format!("up_no_build:{t}")));
    assert!(!g.has("up_no_build:none"));
    let (up, probe, tagged, plain) = (
        pos(&c, &format!("up_no_build:{t}")),
        pos(&c, "probe"),
        pos(&c, &format!("tag_latest:{t}")),
        pos(&c, "up_plain"),
    );
    assert!(up < probe && probe < tagged && tagged < plain, "{c:?}");
    assert_eq!(*g.stamp_at_tag_latest.lock().unwrap(), Some(false));
    assert_eq!(
        read_stamp(d.path()),
        Some(Stamp {
            tag: t,
            key_fp: key_fp(),
            pin_sha: pin()
        })
    );
    assert!(read_backoff(d.path()).is_none());
    assert!(g.rmis().is_empty(), "{c:?}");
}

#[tokio::test]
async fn a_rejected_fresh_node_never_removes_latest_or_a_running_image() {
    // The candidate is in use by a container: it is kept, nothing is removed.
    let d = tempfile::tempdir().unwrap();
    let mut g = G::new(d.path());
    g.probe_ok = false;
    g.running = Some(vec![CAND_ID.to_string()]);
    assert!(run(&g, true).await.is_err());
    assert!(g.has("probe"));
    assert!(g.rmis().is_empty(), "{:?}", g.calls());
    assert!(read_stamp(d.path()).is_none());
    assert_eq!(read_backoff(d.path()).map(|b| b.attempts), Some(1));

    // Containers unknown: kept as well.
    let d = tempfile::tempdir().unwrap();
    let mut g = G::new(d.path());
    g.probe_ok = false;
    g.running = None;
    assert!(run(&g, true).await.is_err());
    assert!(g.has("probe"));
    assert!(g.rmis().is_empty(), "{:?}", g.calls());

    // Not in use: only the candidate id goes, never latest's.
    let d = tempfile::tempdir().unwrap();
    let mut g = G::new(d.path());
    g.probe_ok = false;
    assert!(run(&g, true).await.is_err());
    assert!(g.has("probe"));
    assert_eq!(g.rmis(), s(&[&format!("rmi:{CAND_ID}")]));
    never_removed_latest(&g);
}

#[tokio::test]
async fn a_missing_image_inside_a_backoff_still_builds_and_is_gated() {
    let d = tempfile::tempdir().unwrap();
    record_attempt(d.path(), &tag(), NOW - 10).unwrap();
    assert!(backoff_defers(read_backoff(d.path()).as_ref(), &tag(), NOW));
    let g = G::new(d.path());
    assert_eq!(run(&g, true).await.unwrap(), Outcome::BuiltFresh);
    let t = tag();
    let c = g.calls();
    assert_eq!(g.builds.load(Ordering::SeqCst), 1);
    assert!(g.has("probe"), "{c:?}");
    assert!(
        pos(&c, "probe") < pos(&c, &format!("tag_latest:{t}")),
        "{c:?}"
    );

    let d = tempfile::tempdir().unwrap();
    record_attempt(d.path(), &tag(), NOW - 10).unwrap();
    let mut g = G::new(d.path());
    g.probe_ok = false;
    assert!(run(&g, true).await.is_err());
    assert_eq!(g.builds.load(Ordering::SeqCst), 1);
    assert!(g.has("probe"));
    assert!(!g.has(&format!("tag_latest:{t}")));
    assert_eq!(read_backoff(d.path()).map(|b| b.attempts), Some(2));
}

#[tokio::test]
async fn a_missing_image_with_rebuild_disabled_still_builds_and_is_gated() {
    let d = tempfile::tempdir().unwrap();
    let g = G::new(d.path());
    assert_eq!(run(&g, false).await.unwrap(), Outcome::BuiltFresh);
    let t = tag();
    let c = g.calls();
    assert_eq!(g.builds.load(Ordering::SeqCst), 1);
    assert!(g.has("probe"), "{c:?}");
    assert!(
        pos(&c, "probe") < pos(&c, &format!("tag_latest:{t}")),
        "{c:?}"
    );

    let d = tempfile::tempdir().unwrap();
    let mut g = G::new(d.path());
    g.probe_ok = false;
    assert!(run(&g, false).await.is_err());
    assert_eq!(g.builds.load(Ordering::SeqCst), 1);
    assert!(g.has("probe"));
    assert!(!g.has(&format!("tag_latest:{t}")));
    assert!(!g.has("up_plain"));
    assert!(read_stamp(d.path()).is_none());
}

#[tokio::test]
async fn a_stop_during_the_fresh_probe_promotes_nothing_and_records_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut g = G::new(d.path());
    g.stop_on_probe = true;
    assert_eq!(run(&g, true).await.unwrap(), Outcome::Stopped);
    assert!(g.has("probe"));
    assert!(!g.has(&format!("tag_latest:{}", tag())));
    assert!(!g.has("up_plain"));
    assert!(read_stamp(d.path()).is_none());
    assert!(read_backoff(d.path()).is_none());
}
