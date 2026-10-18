//! A3 (F2 cadence): a fresh start() whose node did not prove the device key is
//! not rebuilt by every supervisor / watcher cycle. A separate cooldown record
//! (never `node-rebuild.backoff`, which the fresh path ignores by design) is
//! written only for a key-proof rejection and read only on the fresh path.
use super::super::diiisco_rebuild::{
    discard_candidate_by_id, fp12, run_node_sequence, stamp_is_current, NodeOps, Outcome,
    LATEST_REF,
};
use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

const NOW: u64 = 3_000_000;
const COOLDOWN_FILE: &str = "node-fresh-reject.cooldown";
const LATEST_ID: &str = "sha256:old";
const CAND_ID: &str = "sha256:new";

fn key_fp() -> String {
    fp12(&format!("sk-fem-{}", "ab".repeat(32)))
}

fn pin() -> String {
    "a".repeat(40)
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

    fn has(&self, what: &str) -> bool {
        self.calls().iter().any(|c| c == what)
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
    fn probe_ran(&self) -> bool {
        self.has("probe")
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

async fn run_at(g: &G, now: u64) -> Result<Outcome> {
    run_node_sequence(g, &g.dir, &key_fp(), &pin(), move || now, true).await
}

fn builds(g: &G) -> usize {
    g.builds.load(Ordering::SeqCst)
}

#[tokio::test]
async fn a_second_fresh_start_inside_the_cooldown_builds_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut g = G::new(d.path());
    g.probe_ok = false;
    assert!(run_at(&g, NOW).await.is_err());
    assert_eq!(builds(&g), 1);

    let err = run_at(&g, NOW + 10).await.unwrap_err();
    assert!(err.to_string().contains("backing off"), "{err}");
    assert_eq!(builds(&g), 1, "{:?}", g.calls());

    // After the first hour the next attempt is allowed.
    assert!(run_at(&g, NOW + 3601).await.is_err());
    assert_eq!(builds(&g), 2);
}

#[tokio::test]
async fn the_cooldown_doubles_with_each_rejection() {
    let d = tempfile::tempdir().unwrap();
    let mut g = G::new(d.path());
    g.probe_ok = false;
    assert!(run_at(&g, NOW).await.is_err());
    assert!(run_at(&g, NOW + 3601).await.is_err());
    assert_eq!(builds(&g), 2);
    // Two rejections: the window is now 2 h.
    assert!(run_at(&g, NOW + 3601 + 3600).await.is_err());
    assert_eq!(builds(&g), 2, "{:?}", g.calls());
    assert!(run_at(&g, NOW + 3601 + 7201).await.is_err());
    assert_eq!(builds(&g), 3);
}

#[tokio::test]
async fn a_failed_start_of_the_candidate_is_not_a_key_rejection_and_sets_no_cooldown() {
    let d = tempfile::tempdir().unwrap();
    let mut g = G::new(d.path());
    g.up_ok = false;
    assert!(run_at(&g, NOW).await.is_err());
    assert!(!d.path().join(COOLDOWN_FILE).exists());
    assert!(run_at(&g, NOW + 10).await.is_err());
    assert_eq!(builds(&g), 2, "Docker trouble must keep retrying");
}

#[tokio::test]
async fn a_proven_node_clears_the_cooldown_and_a_stop_never_sets_one() {
    let d = tempfile::tempdir().unwrap();
    let mut g = G::new(d.path());
    g.probe_ok = false;
    assert!(run_at(&g, NOW).await.is_err());
    assert!(d.path().join(COOLDOWN_FILE).exists());
    g.probe_ok = true;
    assert_eq!(run_at(&g, NOW + 3601).await.unwrap(), Outcome::BuiltFresh);
    assert!(!d.path().join(COOLDOWN_FILE).exists());

    let d = tempfile::tempdir().unwrap();
    let mut g = G::new(d.path());
    g.stop_on_probe = true;
    assert_eq!(run_at(&g, NOW).await.unwrap(), Outcome::Stopped);
    assert!(!d.path().join(COOLDOWN_FILE).exists());
}
