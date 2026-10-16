//! v0.4.38 tests: recipe-folded tags (R-R), failed-candidate cleanup by image
//! id (K2), the F2-gated install() with its backoff (K1), and the Dockerfile
//! patch that stops the node echoing a rejected key (K3). Synthetic values only;
//! the Docker side is a recording fake.
use super::super::diiisco_rebuild::{
    after_fresh_build, backoff_defers, candidate_to_remove, compute_recipe_sha,
    discard_candidate_by_id, drop_previous, fp12, gate_candidate, image_tag, image_tag_with,
    install_backoff_check, install_step, plan_start, read_backoff, read_stamp, recipe_sha,
    record_attempt, reject_candidate, run_node_sequence, stamp_is_current, write_stamp, Backoff,
    Gate, ImageState, InstallStep, NodeOps, Outcome, Plan, Stamp, LATEST_REF, REBUILD_LOCK,
    STAMP_FILE,
};
use anyhow::Result;
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const DOCKERFILE: &str = include_str!("diiisco_deploy/diiisco-node/Dockerfile");
const COMPOSE: &str = include_str!("diiisco_deploy/docker-compose.yml");
const NOW: u64 = 2_000_000;

fn key_fp() -> String {
    fp12(&format!("sk-fem-{}", "ab".repeat(32)))
}

fn pin() -> String {
    "a".repeat(40)
}

fn tag() -> String {
    image_tag(&key_fp(), &pin())
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn non_test(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap_or(src)
}

// ------------------------------------------------------------------ the fake

struct K {
    dir: PathBuf,
    calls: Mutex<Vec<String>>,
    builds: AtomicUsize,
    present: AtomicBool,
    promoted: AtomicBool,
    cand_exists: AtomicBool,
    build_ok: bool,
    build_leaves_image: bool,
    up_ok: bool,
    probe_ok: AtomicBool,
    tag_ok: bool,
    same_id: bool,
    running: Option<Vec<String>>,
    stop: AtomicBool,
    stop_on_build: bool,
    stop_on_probe: bool,
    stamp_at_tag_latest: Mutex<Option<bool>>,
}

impl K {
    fn new(dir: &Path, have_image: bool) -> Self {
        K {
            dir: dir.to_path_buf(),
            calls: Mutex::new(Vec::new()),
            builds: AtomicUsize::new(0),
            present: AtomicBool::new(have_image),
            promoted: AtomicBool::new(false),
            cand_exists: AtomicBool::new(false),
            build_ok: true,
            build_leaves_image: true,
            up_ok: true,
            probe_ok: AtomicBool::new(true),
            tag_ok: true,
            same_id: false,
            running: Some(Vec::new()),
            stop: AtomicBool::new(false),
            stop_on_build: false,
            stop_on_probe: false,
            stamp_at_tag_latest: Mutex::new(None),
        }
    }

    fn rec(&self, c: String) {
        self.calls.lock().unwrap().push(c);
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn rmis(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|c| c.starts_with("rmi:"))
            .collect()
    }
}

#[async_trait]
impl NodeOps for K {
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
            let id = if self.promoted.load(Ordering::SeqCst) {
                "sha256:new"
            } else {
                "sha256:old"
            };
            return Some(id.into());
        }
        if !self.cand_exists.load(Ordering::SeqCst) {
            return None;
        }
        let id = if self.same_id {
            "sha256:old"
        } else {
            "sha256:new"
        };
        Some(id.into())
    }
    fn build(&self, t: &str) -> Result<()> {
        self.rec(format!("build:{t}"));
        self.builds.fetch_add(1, Ordering::SeqCst);
        if self.stop_on_build {
            self.stop.store(true, Ordering::SeqCst);
        }
        if self.build_leaves_image {
            self.cand_exists.store(true, Ordering::SeqCst);
        }
        if self.build_ok {
            Ok(())
        } else {
            anyhow::bail!("fake build failed")
        }
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
        self.probe_ok.load(Ordering::SeqCst)
    }
    fn tag_latest(&self, t: &str) -> Result<()> {
        self.rec(format!("tag_latest:{t}"));
        *self.stamp_at_tag_latest.lock().unwrap() =
            Some(stamp_is_current(&self.dir, &key_fp(), &pin()));
        if !self.tag_ok {
            anyhow::bail!("fake tag failed");
        }
        self.promoted.store(true, Ordering::SeqCst);
        self.present.store(true, Ordering::SeqCst);
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

async fn seq(k: &K) -> Result<Outcome> {
    run_node_sequence(k, &k.dir, &key_fp(), &pin(), || NOW, true).await
}

fn pos(c: &[String], what: &str) -> usize {
    c.iter()
        .position(|x| x == what)
        .unwrap_or_else(|| panic!("missing {what} in {c:?}"))
}

// -------------------------------------------------------------------- R-R

#[test]
fn the_recipe_is_part_of_the_tag() {
    let (k, p) = (key_fp(), pin());
    assert_ne!(
        image_tag_with(&k, &p, &"1".repeat(64)),
        image_tag_with(&k, &p, &"2".repeat(64))
    );
    assert_eq!(image_tag(&k, &p), image_tag_with(&k, &p, recipe_sha()));
    let t = image_tag(&k, &p);
    assert_eq!(t.len(), 16);
    assert!(t.starts_with("fem-"));
}

#[test]
fn recipe_sha_covers_the_embedded_dockerfile_and_compose() {
    let r = recipe_sha();
    assert_eq!(r.len(), 64);
    assert!(r
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    assert_eq!(r, compute_recipe_sha(DOCKERFILE, COMPOSE));
    assert_ne!(
        compute_recipe_sha(DOCKERFILE, COMPOSE),
        compute_recipe_sha(&format!("{DOCKERFILE}#"), COMPOSE)
    );
    assert_ne!(
        compute_recipe_sha(DOCKERFILE, COMPOSE),
        compute_recipe_sha(DOCKERFILE, &format!("{COMPOSE}#"))
    );
}

fn legacy_tag(key_fp: &str, pin: &str) -> String {
    let h = hex::encode(Sha256::digest(format!("{key_fp}{pin}").as_bytes()));
    format!("fem-{}", &h[..12])
}

#[test]
fn a_0_4_37_stamp_is_stale_and_plans_exactly_one_build() {
    let d = tempfile::tempdir().unwrap();
    let (kf, p) = (key_fp(), pin());
    let legacy = Stamp {
        tag: legacy_tag(&kf, &p),
        key_fp: kf.clone(),
        pin_sha: p.clone(),
    };
    assert_ne!(legacy.tag, image_tag(&kf, &p), "tag shape changed");
    write_stamp(d.path(), &legacy).unwrap();
    assert!(!stamp_is_current(d.path(), &kf, &p));
    assert_eq!(
        plan_start(Some(&legacy), None, &kf, &p, NOW),
        Plan::BuildNew {
            tag: image_tag(&kf, &p)
        }
    );
    assert_eq!(
        install_step(ImageState::Present, stamp_is_current(d.path(), &kf, &p)),
        InstallStep::Build
    );
}

#[test]
fn the_stamp_records_the_recipe_and_old_stamps_without_it_still_read() {
    let d = tempfile::tempdir().unwrap();
    let st = Stamp {
        tag: tag(),
        key_fp: key_fp(),
        pin_sha: pin(),
    };
    write_stamp(d.path(), &st).unwrap();
    let text = std::fs::read_to_string(d.path().join(STAMP_FILE)).unwrap();
    assert!(
        text.contains(&format!("recipe_sha={}\n", recipe_sha())),
        "{text}"
    );
    assert_eq!(read_stamp(d.path()), Some(st.clone()));
    std::fs::write(
        d.path().join(STAMP_FILE),
        format!(
            "tag={}\nkey_fp={}\npin_sha={}\n",
            st.tag, st.key_fp, st.pin_sha
        ),
    )
    .unwrap();
    assert_eq!(read_stamp(d.path()), Some(st));
}

// ------------------------------------------------------------- K2 (pure)

#[test]
fn candidate_to_remove_table() {
    let run = vec!["sha256:other".to_string()];
    let run_cand = vec!["sha256:new".to_string()];
    let c = Some("sha256:new");
    assert_eq!(
        candidate_to_remove(c, Some("sha256:old"), Some(run.as_slice())),
        Some("sha256:new".to_string())
    );
    assert_eq!(
        candidate_to_remove(c, None, Some(run.as_slice())),
        Some("sha256:new".to_string()),
        "no latest at all (fresh device)"
    );
    assert_eq!(
        candidate_to_remove(None, Some("sha256:old"), Some(run.as_slice())),
        None
    );
    assert_eq!(
        candidate_to_remove(c, Some("sha256:new"), Some(run.as_slice())),
        None,
        "never latest"
    );
    assert_eq!(
        candidate_to_remove(c, Some("sha256:old"), Some(run_cand.as_slice())),
        None,
        "never the image of a running container"
    );
    assert_eq!(
        candidate_to_remove(c, Some("sha256:old"), None),
        None,
        "unknown running set keeps the image"
    );
}

#[test]
fn discard_removes_by_id_only_and_never_by_ref() {
    let d = tempfile::tempdir().unwrap();
    let k = K::new(d.path(), true);
    k.cand_exists.store(true, Ordering::SeqCst);
    discard_candidate_by_id(&k, &tag());
    let t = tag();
    assert_eq!(
        k.calls(),
        vec![
            format!("image_id:diiisco-node:{t}"),
            "image_id:diiisco-node:latest".to_string(),
            "rmi:sha256:new".to_string()
        ]
    );
}

#[test]
fn discard_keeps_latest_running_and_unknown() {
    let d = tempfile::tempdir().unwrap();
    let mut same = K::new(d.path(), true);
    same.same_id = true;
    same.cand_exists.store(true, Ordering::SeqCst);
    discard_candidate_by_id(&same, &tag());
    assert!(same.rmis().is_empty(), "candidate id == latest id");

    let mut busy = K::new(d.path(), true);
    busy.running = Some(s(&["sha256:new"]));
    busy.cand_exists.store(true, Ordering::SeqCst);
    discard_candidate_by_id(&busy, &tag());
    assert!(busy.rmis().is_empty(), "a running container uses it");

    let mut unknown = K::new(d.path(), true);
    unknown.running = None;
    unknown.cand_exists.store(true, Ordering::SeqCst);
    discard_candidate_by_id(&unknown, &tag());
    assert!(unknown.rmis().is_empty(), "docker ps failed");

    let none = K::new(d.path(), true);
    discard_candidate_by_id(&none, &tag());
    assert!(none.rmis().is_empty(), "no candidate image exists");
}

// ------------------------------------------- K2 (through the sequence)

#[tokio::test]
async fn a_failed_upgrade_build_removes_the_candidate_by_id_after_the_restore() {
    let d = tempfile::tempdir().unwrap();
    let mut k = K::new(d.path(), true);
    k.build_ok = false;
    assert_eq!(seq(&k).await.unwrap(), Outcome::UpgradeFailed);
    let t = tag();
    assert_eq!(
        k.calls(),
        vec![
            "image_id:diiisco-node:latest".to_string(),
            format!("build:{t}"),
            "up_no_build:none".to_string(),
            format!("image_id:diiisco-node:{t}"),
            "image_id:diiisco-node:latest".to_string(),
            "rmi:sha256:new".to_string()
        ]
    );
}

#[tokio::test]
async fn a_failed_build_that_left_no_image_removes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut k = K::new(d.path(), true);
    k.build_ok = false;
    k.build_leaves_image = false;
    seq(&k).await.unwrap();
    assert!(k.rmis().is_empty(), "{:?}", k.calls());
}

#[tokio::test]
async fn a_failed_start_probe_or_promotion_each_remove_the_candidate_after_the_restore() {
    for what in ["up", "probe", "tag"] {
        let d = tempfile::tempdir().unwrap();
        let mut k = K::new(d.path(), true);
        match what {
            "up" => k.up_ok = false,
            "probe" => k.probe_ok.store(false, Ordering::SeqCst),
            _ => k.tag_ok = false,
        }
        assert_eq!(seq(&k).await.unwrap(), Outcome::UpgradeFailed, "{what}");
        let c = k.calls();
        assert_eq!(k.rmis(), s(&["rmi:sha256:new"]), "{what}: {c:?}");
        assert!(
            pos(&c, "up_no_build:none") < pos(&c, "rmi:sha256:new"),
            "{what}: removal only after the previous node is back: {c:?}"
        );
        assert!(read_stamp(d.path()).is_none(), "{what}");
        assert_eq!(
            read_backoff(d.path()).map(|b| b.attempts),
            Some(1),
            "{what}"
        );
    }
}

#[tokio::test]
async fn a_failed_candidate_that_still_runs_is_kept() {
    let d = tempfile::tempdir().unwrap();
    let mut k = K::new(d.path(), true);
    k.probe_ok.store(false, Ordering::SeqCst);
    k.running = Some(s(&["sha256:new"]));
    seq(&k).await.unwrap();
    assert!(k.rmis().is_empty(), "{:?}", k.calls());
}

#[tokio::test]
async fn stop_after_the_build_discards_the_candidate_and_brings_nothing_up() {
    let d = tempfile::tempdir().unwrap();
    let mut k = K::new(d.path(), true);
    k.stop_on_build = true;
    assert_eq!(seq(&k).await.unwrap(), Outcome::Stopped);
    let c = k.calls();
    assert!(!c.iter().any(|x| x.starts_with("up_")), "{c:?}");
    assert_eq!(k.rmis(), s(&["rmi:sha256:new"]), "{c:?}");
}

#[tokio::test]
async fn a_failed_fresh_build_discards_what_it_left() {
    let d = tempfile::tempdir().unwrap();
    let mut k = K::new(d.path(), false);
    k.build_ok = false;
    assert!(seq(&k).await.is_err());
    assert_eq!(k.rmis(), s(&["rmi:sha256:new"]), "{:?}", k.calls());
    assert!(read_stamp(d.path()).is_none());
}

#[tokio::test]
async fn a_successful_upgrade_removes_only_the_previous_image() {
    let d = tempfile::tempdir().unwrap();
    let k = K::new(d.path(), true);
    assert_eq!(seq(&k).await.unwrap(), Outcome::Upgraded);
    assert_eq!(k.rmis(), s(&["rmi:sha256:old"]), "{:?}", k.calls());
}

// ------------------------------------------------------------- K1 (gate)

#[tokio::test]
async fn gate_pass_on_a_fresh_device_is_up_then_probe() {
    let d = tempfile::tempdir().unwrap();
    let k = K::new(d.path(), false);
    let t = tag();
    assert_eq!(gate_candidate(&k, &t, false).await, Gate::Pass);
    assert_eq!(
        k.calls(),
        vec![format!("up_no_build:{t}"), "probe".to_string()]
    );
}

#[tokio::test]
async fn gate_rejection_with_an_image_restores_it_and_discards_the_candidate() {
    let d = tempfile::tempdir().unwrap();
    let k = K::new(d.path(), true);
    k.probe_ok.store(false, Ordering::SeqCst);
    k.cand_exists.store(true, Ordering::SeqCst);
    let t = tag();
    assert_eq!(gate_candidate(&k, &t, true).await, Gate::Rejected);
    assert_eq!(
        k.calls(),
        vec![
            "down".to_string(),
            format!("up_no_build:{t}"),
            "probe".to_string(),
            "down".to_string(),
            "up_no_build:none".to_string(),
            format!("image_id:diiisco-node:{t}"),
            "image_id:diiisco-node:latest".to_string(),
            "rmi:sha256:new".to_string()
        ]
    );
}

#[tokio::test]
async fn gate_rejection_on_a_fresh_device_stops_the_candidate_and_discards_it() {
    let d = tempfile::tempdir().unwrap();
    let k = K::new(d.path(), false);
    k.probe_ok.store(false, Ordering::SeqCst);
    k.cand_exists.store(true, Ordering::SeqCst);
    let t = tag();
    assert_eq!(gate_candidate(&k, &t, false).await, Gate::Rejected);
    assert_eq!(
        k.calls(),
        vec![
            format!("up_no_build:{t}"),
            "probe".to_string(),
            "down".to_string(),
            format!("image_id:diiisco-node:{t}"),
            "image_id:diiisco-node:latest".to_string(),
            "rmi:sha256:new".to_string()
        ]
    );
}

#[tokio::test]
async fn gate_start_failure_is_a_rejection_without_a_probe() {
    let d = tempfile::tempdir().unwrap();
    let mut k = K::new(d.path(), true);
    k.up_ok = false;
    k.cand_exists.store(true, Ordering::SeqCst);
    assert_eq!(gate_candidate(&k, &tag(), true).await, Gate::Rejected);
    let c = k.calls();
    assert!(!c.iter().any(|x| x == "probe"), "{c:?}");
    assert_eq!(k.rmis(), s(&["rmi:sha256:new"]), "{c:?}");
}

#[tokio::test]
async fn gate_after_a_stop_brings_nothing_up() {
    let d = tempfile::tempdir().unwrap();
    let k = K::new(d.path(), true);
    k.cand_exists.store(true, Ordering::SeqCst);
    k.stop.store(true, Ordering::SeqCst);
    assert_eq!(gate_candidate(&k, &tag(), true).await, Gate::Stopped);
    let c = k.calls();
    assert!(
        !c.iter().any(|x| x.starts_with("up_") || x == "down"),
        "{c:?}"
    );
    assert_eq!(k.rmis(), s(&["rmi:sha256:new"]));
}

#[tokio::test]
async fn reject_after_a_stop_only_discards() {
    let d = tempfile::tempdir().unwrap();
    let k = K::new(d.path(), true);
    k.cand_exists.store(true, Ordering::SeqCst);
    k.stop.store(true, Ordering::SeqCst);
    reject_candidate(&k, &tag(), true);
    let c = k.calls();
    assert!(!c.iter().any(|x| x.starts_with("up_")), "{c:?}");
    assert!(pos(&c, "down") < pos(&c, "rmi:sha256:new"), "{c:?}");
    assert_eq!(k.rmis(), s(&["rmi:sha256:new"]));
}

#[test]
fn drop_previous_removes_only_a_different_previous_id() {
    let d = tempfile::tempdir().unwrap();
    let k = K::new(d.path(), true);
    k.cand_exists.store(true, Ordering::SeqCst);
    drop_previous(&k, None, &tag());
    assert!(k.calls().is_empty(), "no previous image, nothing to do");
    drop_previous(&k, Some("sha256:new"), &tag());
    assert!(k.rmis().is_empty(), "previous == candidate");
    drop_previous(&k, Some("sha256:old"), &tag());
    assert_eq!(k.rmis(), s(&["rmi:sha256:old"]));
}

// ------------------------------------------------------ K1 (backoff, pure)

#[test]
fn backoff_defers_table() {
    let t = tag();
    let b = |attempts: u32, last_unix: u64, tag: &str| Backoff {
        attempts,
        last_unix,
        tag: tag.to_string(),
    };
    assert!(!backoff_defers(None, &t, NOW));
    assert!(backoff_defers(Some(&b(1, NOW - 10, &t)), &t, NOW));
    assert!(
        !backoff_defers(Some(&b(1, NOW - 3600, &t)), &t, NOW),
        "elapsed"
    );
    assert!(
        backoff_defers(Some(&b(2, NOW - 3601, &t)), &t, NOW),
        "2nd delay is 2 h"
    );
    assert!(!backoff_defers(Some(&b(2, NOW - 7200, &t)), &t, NOW));
    assert!(
        !backoff_defers(Some(&b(1, NOW - 10, "fem-000000000000")), &t, NOW),
        "other tag"
    );
    assert!(
        !backoff_defers(Some(&b(1, NOW + 500, &t)), &t, NOW),
        "future stamp"
    );
}

#[test]
fn install_backoff_check_reads_the_recorded_attempt() {
    let d = tempfile::tempdir().unwrap();
    let t = tag();
    assert!(install_backoff_check(d.path(), &t, NOW).is_ok());
    record_attempt(d.path(), &t, NOW).unwrap();
    let e = install_backoff_check(d.path(), &t, NOW + 10).unwrap_err();
    assert!(e.to_string().contains("backing off"), "{e}");
    assert!(install_backoff_check(d.path(), &t, NOW + 3600).is_ok());
}

// --------------------------------------------- K1 (install()'s locked flow)

#[derive(Debug, PartialEq, Eq)]
enum Flow {
    Built,
    Already,
    Stopped,
}

/// install()'s locked section, driven through the same steps it calls.
async fn install_flow(k: &K, now: u64) -> Result<Flow> {
    let _g = REBUILD_LOCK.lock().await;
    let (kf, p, t) = (key_fp(), pin(), tag());
    let state = k.image_state();
    match install_step(state, stamp_is_current(&k.dir, &kf, &p)) {
        InstallStep::Build => {}
        InstallStep::AlreadyBuilt => return Ok(Flow::Already),
        InstallStep::Unknown => anyhow::bail!("image state unknown"),
    }
    install_backoff_check(&k.dir, &t, now)?;
    let have_image = state == ImageState::Present;
    let prev = if have_image {
        k.image_id(LATEST_REF)
    } else {
        None
    };
    if let Err(e) = k.build(&t) {
        record_attempt(&k.dir, &t, now)?;
        k.discard_candidate(&t);
        return Err(e);
    }
    record_attempt(&k.dir, &t, now)?;
    match gate_candidate(k, &t, have_image).await {
        Gate::Pass => {
            if let Err(e) = after_fresh_build(k, &k.dir, &t, &kf, &p) {
                reject_candidate(k, &t, have_image);
                return Err(e);
            }
            drop_previous(k, prev.as_deref(), &t);
            Ok(Flow::Built)
        }
        Gate::Rejected => anyhow::bail!("candidate rejected"),
        Gate::Stopped => Ok(Flow::Stopped),
    }
}

#[tokio::test]
async fn install_with_an_image_and_a_stale_stamp_is_gated_then_promoted() {
    let d = tempfile::tempdir().unwrap();
    let (kf, p) = (key_fp(), pin());
    let legacy = Stamp {
        tag: legacy_tag(&kf, &p),
        key_fp: kf.clone(),
        pin_sha: p.clone(),
    };
    write_stamp(d.path(), &legacy).unwrap();
    let k = K::new(d.path(), true);
    assert_eq!(install_flow(&k, NOW).await.unwrap(), Flow::Built);
    let t = tag();
    assert_eq!(
        k.calls(),
        vec![
            "image_id:diiisco-node:latest".to_string(),
            format!("build:{t}"),
            "down".to_string(),
            format!("up_no_build:{t}"),
            "probe".to_string(),
            format!("tag_latest:{t}"),
            format!("image_id:diiisco-node:{t}"),
            "rmi:sha256:old".to_string()
        ]
    );
    assert_eq!(*k.stamp_at_tag_latest.lock().unwrap(), Some(false));
    assert!(stamp_is_current(d.path(), &kf, &p));
    assert!(read_backoff(d.path()).is_none(), "cleared on success");
}

#[tokio::test]
async fn install_f2_failure_with_an_image_keeps_the_old_node_and_records_a_backoff() {
    for what in ["probe", "up"] {
        let d = tempfile::tempdir().unwrap();
        let mut k = K::new(d.path(), true);
        match what {
            "probe" => k.probe_ok.store(false, Ordering::SeqCst),
            _ => k.up_ok = false,
        }
        assert!(install_flow(&k, NOW).await.is_err(), "{what}");
        let c = k.calls();
        assert!(
            !c.iter().any(|x| x.starts_with("tag_latest")),
            "{what}: {c:?}"
        );
        assert_eq!(
            c.last().map(String::as_str),
            Some("rmi:sha256:new"),
            "{what}: {c:?}"
        );
        assert!(
            pos(&c, "up_no_build:none") < pos(&c, "rmi:sha256:new"),
            "{what}"
        );
        assert!(read_stamp(d.path()).is_none(), "{what}");
        assert_eq!(
            read_backoff(d.path()).map(|b| b.attempts),
            Some(1),
            "{what}"
        );
        assert!(k.present.load(Ordering::SeqCst));
        assert!(!k.promoted.load(Ordering::SeqCst), "latest never moved");
    }
}

#[tokio::test]
async fn a_fresh_install_runs_the_gate_before_tagging() {
    let d = tempfile::tempdir().unwrap();
    let k = K::new(d.path(), false);
    assert_eq!(install_flow(&k, NOW).await.unwrap(), Flow::Built);
    let t = tag();
    assert_eq!(
        k.calls(),
        vec![
            format!("build:{t}"),
            format!("up_no_build:{t}"),
            "probe".to_string(),
            format!("tag_latest:{t}")
        ]
    );
    assert_eq!(*k.stamp_at_tag_latest.lock().unwrap(), Some(false));
    assert!(stamp_is_current(d.path(), &key_fp(), &pin()));
}

#[tokio::test]
async fn a_fresh_install_that_fails_f2_leaves_no_stamp_no_latest_and_no_candidate() {
    let d = tempfile::tempdir().unwrap();
    let k = K::new(d.path(), false);
    k.probe_ok.store(false, Ordering::SeqCst);
    assert!(install_flow(&k, NOW).await.is_err());
    let t = tag();
    assert_eq!(
        k.calls(),
        vec![
            format!("build:{t}"),
            format!("up_no_build:{t}"),
            "probe".to_string(),
            "down".to_string(),
            format!("image_id:diiisco-node:{t}"),
            "image_id:diiisco-node:latest".to_string(),
            "rmi:sha256:new".to_string()
        ]
    );
    assert!(read_stamp(d.path()).is_none());
    assert_eq!(read_backoff(d.path()).map(|b| b.attempts), Some(1));
    assert!(!k.present.load(Ordering::SeqCst), "no latest was created");
}

#[tokio::test]
async fn a_failed_promotion_restores_the_old_node_and_discards_the_candidate() {
    let d = tempfile::tempdir().unwrap();
    let mut k = K::new(d.path(), true);
    k.tag_ok = false;
    assert!(install_flow(&k, NOW).await.is_err());
    let c = k.calls();
    assert!(
        pos(&c, "up_no_build:none") < pos(&c, "rmi:sha256:new"),
        "{c:?}"
    );
    assert_eq!(k.rmis(), s(&["rmi:sha256:new"]));
    assert!(read_stamp(d.path()).is_none());
}

#[tokio::test]
async fn install_build_failure_discards_what_it_left() {
    let d = tempfile::tempdir().unwrap();
    let mut k = K::new(d.path(), false);
    k.build_ok = false;
    assert!(install_flow(&k, NOW).await.is_err());
    assert_eq!(k.rmis(), s(&["rmi:sha256:new"]), "{:?}", k.calls());
    assert_eq!(read_backoff(d.path()).map(|b| b.attempts), Some(1));
}

#[tokio::test]
async fn a_stop_during_the_gate_promotes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut k = K::new(d.path(), true);
    k.stop_on_probe = true;
    assert_eq!(install_flow(&k, NOW).await.unwrap(), Flow::Stopped);
    let c = k.calls();
    assert!(!c.iter().any(|x| x.starts_with("tag_latest")), "{c:?}");
    assert_eq!(k.rmis(), s(&["rmi:sha256:new"]), "{c:?}");
    assert!(read_stamp(d.path()).is_none());
}

#[tokio::test]
async fn a_failed_fresh_install_is_not_rebuilt_on_every_recovery_cycle() {
    let d = tempfile::tempdir().unwrap();
    let k = K::new(d.path(), false);
    k.probe_ok.store(false, Ordering::SeqCst);
    assert!(install_flow(&k, NOW).await.is_err());
    assert_eq!(k.builds.load(Ordering::SeqCst), 1);
    // the startup recovery and the Docker watcher call install() again
    for dt in [10, 60, 3599] {
        let e = install_flow(&k, NOW + dt).await.unwrap_err();
        assert!(e.to_string().contains("backing off"), "{e}");
    }
    assert_eq!(k.builds.load(Ordering::SeqCst), 1, "no additional build");
    // once the backoff ages: exactly one more build, which backs off again
    assert!(install_flow(&k, NOW + 3601).await.is_err());
    assert_eq!(k.builds.load(Ordering::SeqCst), 2);
    assert_eq!(read_backoff(d.path()).map(|b| b.attempts), Some(2));
    assert!(install_flow(&k, NOW + 3601 + 7199).await.is_err());
    assert_eq!(k.builds.load(Ordering::SeqCst), 2, "2nd delay is 2 h");
    // a pass after the delay promotes and clears the backoff
    k.probe_ok.store(true, Ordering::SeqCst);
    assert_eq!(
        install_flow(&k, NOW + 3601 + 7200).await.unwrap(),
        Flow::Built
    );
    assert_eq!(k.builds.load(Ordering::SeqCst), 3);
    assert!(read_backoff(d.path()).is_none());
    assert_eq!(
        install_flow(&k, NOW + 100_000).await.unwrap(),
        Flow::Already
    );
    assert_eq!(k.builds.load(Ordering::SeqCst), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_installs_and_start_build_once_through_the_gate() {
    let d = tempfile::tempdir().unwrap();
    let k = Arc::new(K::new(d.path(), false));
    let work = async {
        let (a, b, c) = (Arc::clone(&k), Arc::clone(&k), Arc::clone(&k));
        let i1 = tokio::spawn(async move { install_flow(&a, NOW).await.unwrap() });
        let i2 = tokio::spawn(async move { install_flow(&b, NOW).await.unwrap() });
        let st = tokio::spawn(async move { seq(&c).await.unwrap() });
        let _ = (i1.await.unwrap(), i2.await.unwrap(), st.await.unwrap());
    };
    tokio::time::timeout(std::time::Duration::from_secs(20), work)
        .await
        .expect("no deadlock");
    let calls = k.calls();
    assert_eq!(k.builds.load(Ordering::SeqCst), 1, "{calls:?}");
    let latest = format!("tag_latest:{}", tag());
    assert_eq!(
        calls.iter().filter(|c| **c == latest).count(),
        1,
        "{calls:?}"
    );
    assert!(k.rmis().is_empty(), "{calls:?}");
}

// -------------------------------------------------------- source structure

#[test]
fn install_runs_backoff_and_gate_in_order_inside_the_lock() {
    let body = non_test(include_str!("diiisco.rs"));
    let install = body.split("async fn install(&self)").nth(1).unwrap();
    let install = install.split("async fn start(").next().unwrap();
    let at = |needle: &str| {
        install
            .find(needle)
            .unwrap_or_else(|| panic!("install() lacks {needle}"))
    };
    let order = [
        "REBUILD_LOCK.lock()",
        "install_step(",
        "install_backoff_check(",
        "\"compose\", \"build\"",
        "record_attempt(",
        "gate_candidate(",
        "after_fresh_build(",
        "drop_previous(",
    ];
    let idx: Vec<usize> = order.iter().map(|n| at(n)).collect();
    assert!(idx.windows(2).all(|w| w[0] < w[1]), "{order:?} -> {idx:?}");
    assert!(install.contains("reject_candidate("));
    assert!(install.contains("discard_candidate("));
}

#[test]
fn every_failed_upgrade_exit_discards_its_candidate() {
    let seq_src = non_test(include_str!("diiisco_rebuild.rs"));
    assert!(!seq_src.contains("fallback_old(ops)"));
    assert_eq!(seq_src.matches("fallback_old(ops, Some(").count(), 4);
    assert_eq!(seq_src.matches("fallback_old(ops, None)").count(), 2);
}

/// Drop all whitespace so a scan survives rustfmt wrapping (needles go through
/// the same function).
fn squash(text: &str) -> String {
    text.split_whitespace().collect()
}

#[test]
fn docker_node_uses_ps_inspect_and_rmi_by_id_only() {
    let body = squash(non_test(include_str!("diiisco.rs")));
    assert!(body.contains(&squash("self.docker(&[\"ps\", \"-q\"])")));
    assert!(body.contains(&squash("vec![\"inspect\", \"--format\", \"{{.Image}}\"]")));
    assert!(body.contains(&squash(
        "self.docker(&[\"images\", \"-q\", \"--no-trunc\", reference])"
    )));
    assert_eq!(body.matches("\"rmi\"").count(), 1);
    assert!(body.contains(&squash("self.docker(&[\"rmi\", id])")));
}

#[test]
fn install_records_its_attempt_only_after_the_build_has_run() {
    let body = non_test(include_str!("diiisco.rs"));
    let install = body.split("async fn install(&self)").nth(1).unwrap();
    let install = install.split("async fn start(").next().unwrap();
    let build = install.find("\"compose\", \"build\"").unwrap();
    for needle in ["record_attempt(", "record_install_attempt("] {
        for (i, _) in install.match_indices(needle) {
            assert!(build < i, "{needle} at {i} precedes the build at {build}");
        }
        assert!(install.contains(needle), "{needle} missing");
    }
}

// ------------------------------------------------------- K3 (Dockerfile)

#[test]
fn dockerfile_patches_the_key_echo_and_asserts_it_in_source_and_dist() {
    let sed = "sed -i 's#\"message\": \"Incorrect API key provided: \" + token,#\"message\": \"Incorrect API key provided.\",#' src/utils/endpoint.ts";
    assert_eq!(DOCKERFILE.matches(sed).count(), 1);
    assert!(DOCKERFILE.contains(
        "grep -qF '\"message\": \"Incorrect API key provided.\",' src/utils/endpoint.ts"
    ));
    assert!(DOCKERFILE.contains("! grep -rqF 'Incorrect API key provided: ' src"));
    assert!(DOCKERFILE.contains("! grep -rqF 'Incorrect API key provided: ' dist"));
    assert!(DOCKERFILE.contains("grep -rqF 'Incorrect API key provided.' dist"));
    let patch = DOCKERFILE.find(sed).unwrap();
    let compile = DOCKERFILE.find("# Compile TypeScript").unwrap();
    let build = DOCKERFILE.find("RUN (npm run build").unwrap();
    let dist = DOCKERFILE
        .find("RUN ! grep -rqF 'Incorrect API key provided: ' dist")
        .unwrap();
    let entry = DOCKERFILE.find("RUN test -f dist/dev.js").unwrap();
    let checkout = DOCKERFILE.find("git checkout --detach FETCH_HEAD").unwrap();
    assert!(
        checkout < patch && patch < compile,
        "patch sits between checkout and build"
    );
    assert!(build < dist && dist < entry, "dist check follows the build");
    assert!(!DOCKERFILE.contains("ts-node"));
    assert_eq!(
        DOCKERFILE
            .matches("\"Incorrect API key provided: \" + token")
            .count(),
        1,
        "the original text appears only as the sed pattern"
    );
}

#[cfg(target_os = "linux")]
fn run_block(starts_with: &str) -> String {
    let at = DOCKERFILE.find(starts_with).expect("block present");
    let block = DOCKERFILE[at..].split("\n\n").next().unwrap_or("");
    let joined = block
        .lines()
        .map(|l| l.trim().trim_end_matches('\\').trim())
        .collect::<Vec<_>>()
        .join(" ");
    joined
        .strip_prefix("RUN ")
        .expect("a RUN block")
        .to_string()
}

#[cfg(target_os = "linux")]
fn sh_ok(script: &str, dir: &Path) -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn write(dir: &Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

#[cfg(target_os = "linux")]
const UPSTREAM: &str = "  if (!allowedKeys().has(token)) return res.status(401).json({\n    \"error\": {\n      \"message\": \"Incorrect API key provided: \" + token,\n      \"type\": \"authentication_error\"\n    }\n  });\n";

#[cfg(target_os = "linux")]
#[test]
fn the_source_patch_runs_and_every_miss_fails_the_build() {
    let script = run_block("RUN sed -i 's#\"message\"");
    let ok = tempfile::tempdir().unwrap();
    write(ok.path(), "src/utils/endpoint.ts", UPSTREAM);
    assert!(sh_ok(&script, ok.path()), "{script}");
    let got = std::fs::read_to_string(ok.path().join("src/utils/endpoint.ts")).unwrap();
    assert!(
        got.contains("      \"message\": \"Incorrect API key provided.\",\n"),
        "{got}"
    );
    assert!(!got.contains("provided: "));

    let gone = tempfile::tempdir().unwrap();
    write(
        gone.path(),
        "src/utils/endpoint.ts",
        "  \"message\": \"changed upstream\",\n",
    );
    assert!(
        !sh_ok(&script, gone.path()),
        "patched text absent must fail"
    );

    let twice = tempfile::tempdir().unwrap();
    write(twice.path(), "src/utils/endpoint.ts", UPSTREAM);
    write(
        twice.path(),
        "src/other.ts",
        "x(\"Incorrect API key provided: \" + y)\n",
    );
    assert!(
        !sh_ok(&script, twice.path()),
        "a second echo site must fail"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn the_dist_check_passes_only_a_patched_build() {
    let script = run_block("RUN ! grep -rqF 'Incorrect API key provided: ' dist");
    let good = tempfile::tempdir().unwrap();
    write(
        good.path(),
        "dist/utils/endpoint.js",
        "m(\"message\": \"Incorrect API key provided.\",\n",
    );
    assert!(sh_ok(&script, good.path()), "{script}");

    let echoing = tempfile::tempdir().unwrap();
    write(
        echoing.path(),
        "dist/utils/endpoint.js",
        "\"message\": \"Incorrect API key provided: \" + token,\n",
    );
    assert!(!sh_ok(&script, echoing.path()));

    let mixed = tempfile::tempdir().unwrap();
    write(
        mixed.path(),
        "dist/a.js",
        "\"Incorrect API key provided.\"\n",
    );
    write(
        mixed.path(),
        "dist/b.js",
        "\"Incorrect API key provided: \" + t\n",
    );
    assert!(!sh_ok(&script, mixed.path()));

    let empty = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(empty.path().join("dist")).unwrap();
    assert!(
        !sh_ok(&script, empty.path()),
        "no patched text in dist must fail"
    );
}
