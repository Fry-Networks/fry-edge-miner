//! One-time gated rebuild tests. Synthetic values only; key values are never
//! printed in assertion messages. The Docker side is a recording fake.
use super::super::diiisco_rebuild::{
    after_fresh_build, clear_backoff, delay_secs, fp12, image_tag, pin_sha, pin_sha_of, plan_start,
    probe_verdict, read_backoff, read_stamp, record_attempt, redact_node_output, run_node_sequence,
    write_embedded_dockerfile, write_stamp, Backoff, NodeOps, Outcome, Plan, Stamp, BACKOFF_FILE,
    REBUILD_LOCK, STAMP_FILE,
};
use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const DOCKERFILE: &str = include_str!("diiisco_deploy/diiisco-node/Dockerfile");
const COMPOSE: &str = include_str!("diiisco_deploy/docker-compose.yml");
const NOW: u64 = 1_000_000;

fn key() -> String {
    format!("sk-fem-{}", "ab".repeat(32))
}

fn key_fp() -> String {
    fp12(&key())
}

fn pin() -> String {
    "a".repeat(40)
}

fn tag() -> String {
    image_tag(&key_fp(), &pin())
}

fn stamp_for(kfp: &str, p: &str) -> Stamp {
    Stamp {
        tag: image_tag(kfp, p),
        key_fp: kfp.to_string(),
        pin_sha: p.to_string(),
    }
}

// ---------------------------------------------------------------- plan_start

#[test]
fn tag_and_fingerprint_shapes() {
    let t = tag();
    assert_eq!(t.len(), 4 + 12, "{t}");
    assert!(t.starts_with("fem-"), "{t}");
    assert_eq!(key_fp().len(), 12);
    assert!(key_fp().bytes().all(|b| b.is_ascii_hexdigit()));
    assert!(image_tag(&key_fp(), &pin()) != image_tag(&key_fp(), &"b".repeat(40)));
    assert!(image_tag(&key_fp(), &pin()) != image_tag(&fp12("other"), &pin()));
}

#[test]
fn legacy_device_without_a_stamp_gets_exactly_one_build() {
    let p = plan_start(None, None, &key_fp(), &pin(), NOW);
    assert_eq!(p, Plan::BuildNew { tag: tag() });
}

#[test]
fn a_matching_stamp_runs_the_existing_image() {
    let s = stamp_for(&key_fp(), &pin());
    assert_eq!(
        plan_start(Some(&s), None, &key_fp(), &pin(), NOW),
        Plan::RunExisting
    );
}

#[test]
fn a_different_pin_triggers_one_build() {
    let s = stamp_for(&key_fp(), &"b".repeat(40));
    assert_eq!(
        plan_start(Some(&s), None, &key_fp(), &pin(), NOW),
        Plan::BuildNew { tag: tag() }
    );
}

#[test]
fn a_different_key_triggers_a_build() {
    let s = stamp_for(&fp12("another-key"), &pin());
    assert_eq!(
        plan_start(Some(&s), None, &key_fp(), &pin(), NOW),
        Plan::BuildNew { tag: tag() }
    );
}

#[test]
fn an_attempt_inside_the_backoff_defers_to_the_old_image() {
    let b = Backoff {
        attempts: 1,
        last_unix: NOW - 10,
        tag: tag(),
    };
    assert_eq!(
        plan_start(None, Some(&b), &key_fp(), &pin(), NOW),
        Plan::RunOldDeferred
    );
}

#[test]
fn after_the_backoff_a_build_is_due_again() {
    let b = Backoff {
        attempts: 1,
        last_unix: NOW - 3601,
        tag: tag(),
    };
    assert_eq!(
        plan_start(None, Some(&b), &key_fp(), &pin(), NOW),
        Plan::BuildNew { tag: tag() }
    );
}

#[test]
fn a_backoff_for_another_target_or_the_future_is_ignored() {
    let other = Backoff {
        attempts: 3,
        last_unix: NOW - 10,
        tag: image_tag(&key_fp(), &"c".repeat(40)),
    };
    assert_eq!(
        plan_start(None, Some(&other), &key_fp(), &pin(), NOW),
        Plan::BuildNew { tag: tag() }
    );
    let future = Backoff {
        attempts: 1,
        last_unix: NOW + 500,
        tag: tag(),
    };
    assert_eq!(
        plan_start(None, Some(&future), &key_fp(), &pin(), NOW),
        Plan::BuildNew { tag: tag() }
    );
}

#[test]
fn backoff_delay_doubles_from_one_hour_to_a_day() {
    let got: Vec<u64> = (1..=8).map(delay_secs).collect();
    assert_eq!(
        got,
        vec![3600, 7200, 14400, 28800, 57600, 86400, 86400, 86400]
    );
    assert_eq!(delay_secs(u32::MAX), 86400);
}

// ------------------------------------------------------------- state files

#[test]
fn stamp_round_trips_and_leaves_no_tmp_file() {
    let d = tempfile::tempdir().unwrap();
    let s = stamp_for(&key_fp(), &pin());
    write_stamp(d.path(), &s).unwrap();
    assert_eq!(read_stamp(d.path()), Some(s));
    let names: Vec<String> = std::fs::read_dir(d.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec![STAMP_FILE.to_string()]);
}

#[test]
fn a_garbage_stamp_is_absent_and_plans_one_build() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join(STAMP_FILE), "\u{0}\u{1}not a stamp").unwrap();
    let stamp = read_stamp(d.path());
    assert!(stamp.is_none());
    assert_eq!(
        plan_start(stamp.as_ref(), None, &key_fp(), &pin(), NOW),
        Plan::BuildNew { tag: tag() }
    );
    std::fs::write(
        d.path().join(STAMP_FILE),
        "tag=fem-zzzz\nkey_fp=1\npin_sha=2\n",
    )
    .unwrap();
    assert!(read_stamp(d.path()).is_none(), "bad shapes are absent");
}

#[test]
fn a_garbage_backoff_is_absent() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join(BACKOFF_FILE), "attempts=banana\n").unwrap();
    assert!(read_backoff(d.path()).is_none());
    std::fs::write(d.path().join(BACKOFF_FILE), [0xffu8, 0xfe, 0x00]).unwrap();
    assert!(read_backoff(d.path()).is_none());
}

#[test]
fn recording_attempts_counts_per_target_and_clears() {
    let d = tempfile::tempdir().unwrap();
    record_attempt(d.path(), &tag(), NOW).unwrap();
    record_attempt(d.path(), &tag(), NOW + 5).unwrap();
    let b = read_backoff(d.path());
    assert_eq!(
        b,
        Some(Backoff {
            attempts: 2,
            last_unix: NOW + 5,
            tag: tag()
        })
    );
    let other = image_tag(&key_fp(), &"c".repeat(40));
    record_attempt(d.path(), &other, NOW + 9).unwrap();
    assert_eq!(read_backoff(d.path()).map(|b| b.attempts), Some(1));
    clear_backoff(d.path());
    assert!(read_backoff(d.path()).is_none());
    clear_backoff(d.path());
}

// ------------------------------------------------------------ probe verdict

#[test]
fn probe_verdict_needs_200_with_the_key_and_401_without() {
    assert!(probe_verdict(200, 401));
    assert!(!probe_verdict(200, 200), "override not loaded");
    assert!(!probe_verdict(401, 401), "node rejects the device key");
    assert!(!probe_verdict(0, 0), "connection refused");
    assert!(!probe_verdict(401, 200));
    assert!(!probe_verdict(200, 0));
    assert!(!probe_verdict(500, 401));
}

// ---------------------------------------------------------------- the fake

struct Fake {
    dir: PathBuf,
    calls: Mutex<Vec<String>>,
    builds: AtomicUsize,
    have_image: bool,
    build_ok: bool,
    refresh_ok: bool,
    probe_ok: bool,
    same_id: bool,
    stop: AtomicBool,
    stop_on_build: bool,
    stamp_at_tag_latest: Mutex<Option<bool>>,
    stamp_at_rmi: Mutex<Option<bool>>,
}

impl Fake {
    fn new(dir: &Path) -> Self {
        Fake {
            dir: dir.to_path_buf(),
            calls: Mutex::new(Vec::new()),
            builds: AtomicUsize::new(0),
            have_image: true,
            build_ok: true,
            refresh_ok: true,
            probe_ok: true,
            same_id: false,
            stop: AtomicBool::new(false),
            stop_on_build: false,
            stamp_at_tag_latest: Mutex::new(None),
            stamp_at_rmi: Mutex::new(None),
        }
    }

    fn rec(&self, s: String) {
        self.calls.lock().unwrap().push(s);
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl NodeOps for Fake {
    fn image_exists(&self) -> bool {
        self.have_image
    }
    fn refresh_dockerfile(&self) -> Result<()> {
        self.rec("refresh_dockerfile".into());
        if self.refresh_ok {
            Ok(())
        } else {
            anyhow::bail!("fake refresh failed")
        }
    }
    fn image_id(&self, reference: &str) -> Option<String> {
        self.rec(format!("image_id:{reference}"));
        if reference == "diiisco-node:latest" || self.same_id {
            Some("sha256:old".into())
        } else {
            Some("sha256:new".into())
        }
    }
    fn build(&self, t: &str) -> Result<()> {
        self.rec(format!("build:{t}"));
        self.builds.fetch_add(1, Ordering::SeqCst);
        if self.stop_on_build {
            self.stop.store(true, Ordering::SeqCst);
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
        Ok(())
    }
    async fn probe(&self) -> bool {
        self.rec("probe".into());
        self.probe_ok
    }
    fn tag_latest(&self, t: &str) -> Result<()> {
        self.rec(format!("tag_latest:{t}"));
        *self.stamp_at_tag_latest.lock().unwrap() = Some(read_stamp(&self.dir).is_some());
        Ok(())
    }
    fn rmi_by_id(&self, id: &str) {
        self.rec(format!("rmi:{id}"));
        *self.stamp_at_rmi.lock().unwrap() = Some(read_stamp(&self.dir).is_some());
    }
    fn stop_requested(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

async fn run(f: &Fake) -> Result<Outcome> {
    run_node_sequence(f, &f.dir, &key_fp(), &pin(), || NOW).await
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

// ------------------------------------------------------------ the sequence

#[tokio::test]
async fn upgrade_switch_order_is_build_up_probe_tag_stamp_rmi_by_id() {
    let d = tempfile::tempdir().unwrap();
    let f = Fake::new(d.path());
    let out = run(&f).await.unwrap();
    assert_eq!(out, Outcome::Upgraded);
    let t = tag();
    let want = vec![
        "image_id:diiisco-node:latest".to_string(),
        "refresh_dockerfile".to_string(),
        format!("build:{t}"),
        "down".to_string(),
        format!("up_no_build:{t}"),
        "probe".to_string(),
        format!("tag_latest:{t}"),
        format!("image_id:diiisco-node:{t}"),
        "rmi:sha256:old".to_string(),
    ];
    assert_eq!(f.calls(), want);
    assert_eq!(*f.stamp_at_tag_latest.lock().unwrap(), Some(false));
    assert_eq!(*f.stamp_at_rmi.lock().unwrap(), Some(true));
    assert_eq!(read_stamp(d.path()), Some(stamp_for(&key_fp(), &pin())));
    assert!(
        read_backoff(d.path()).is_none(),
        "backoff cleared on success"
    );
    assert!(
        !f.calls().iter().any(|c| c.contains("rmi:diiisco-node")),
        "never rmi latest by name"
    );
}

#[tokio::test]
async fn the_previous_image_is_not_removed_when_it_is_the_new_one() {
    let d = tempfile::tempdir().unwrap();
    let mut f = Fake::new(d.path());
    f.same_id = true;
    run(&f).await.unwrap();
    assert!(!f.calls().iter().any(|c| c.starts_with("rmi:")));
}

#[tokio::test]
async fn a_failed_build_keeps_the_old_image_with_no_stamp_and_a_backoff() {
    let d = tempfile::tempdir().unwrap();
    let mut f = Fake::new(d.path());
    f.build_ok = false;
    let out = run(&f).await.unwrap();
    assert_eq!(out, Outcome::UpgradeFailed);
    let t = tag();
    assert_eq!(
        f.calls(),
        vec![
            "image_id:diiisco-node:latest".to_string(),
            "refresh_dockerfile".to_string(),
            format!("build:{t}"),
            "up_no_build:none".to_string()
        ]
    );
    assert!(read_stamp(d.path()).is_none());
    assert_eq!(read_backoff(d.path()).map(|b| b.attempts), Some(1));
}

#[tokio::test]
async fn a_failed_probe_restores_the_old_image_and_promotes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut f = Fake::new(d.path());
    f.probe_ok = false;
    let out = run(&f).await.unwrap();
    assert_eq!(out, Outcome::UpgradeFailed);
    let c = f.calls();
    assert_eq!(c.last().map(String::as_str), Some("up_no_build:none"));
    assert!(!c.iter().any(|x| x.starts_with("tag_latest")));
    assert!(!c.iter().any(|x| x.starts_with("rmi")));
    assert!(read_stamp(d.path()).is_none());
    assert_eq!(read_backoff(d.path()).map(|b| b.attempts), Some(1));
}

#[tokio::test]
async fn stop_during_the_build_brings_nothing_up_and_promotes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut f = Fake::new(d.path());
    f.stop_on_build = true;
    let out = run(&f).await.unwrap();
    assert_eq!(out, Outcome::Stopped);
    let c = f.calls();
    assert!(!c.iter().any(|x| x.starts_with("up_")), "{c:?}");
    assert!(!c.iter().any(|x| x.starts_with("tag_latest")), "{c:?}");
    assert!(read_stamp(d.path()).is_none());
    assert_eq!(
        read_backoff(d.path()).map(|b| b.attempts),
        Some(1),
        "the interrupted attempt stays recorded"
    );
}

#[tokio::test]
async fn stop_before_a_plain_up_skips_it() {
    let d = tempfile::tempdir().unwrap();
    write_stamp(d.path(), &stamp_for(&key_fp(), &pin())).unwrap();
    let f = Fake::new(d.path());
    f.stop.store(true, Ordering::SeqCst);
    let out = run(&f).await.unwrap();
    assert_eq!(out, Outcome::Stopped);
    assert!(!f.calls().iter().any(|x| x.starts_with("up_")));
}

#[tokio::test]
async fn a_current_stamp_runs_the_existing_image_without_building() {
    let d = tempfile::tempdir().unwrap();
    write_stamp(d.path(), &stamp_for(&key_fp(), &pin())).unwrap();
    let f = Fake::new(d.path());
    assert_eq!(run(&f).await.unwrap(), Outcome::Ran);
    assert_eq!(f.builds.load(Ordering::SeqCst), 0);
    assert_eq!(f.calls(), s(&["down", "up_plain"]));
}

#[tokio::test]
async fn inside_the_backoff_the_old_image_runs_with_no_build() {
    let d = tempfile::tempdir().unwrap();
    record_attempt(d.path(), &tag(), NOW - 10).unwrap();
    let f = Fake::new(d.path());
    assert_eq!(run(&f).await.unwrap(), Outcome::Ran);
    assert_eq!(f.builds.load(Ordering::SeqCst), 0);
    assert_eq!(f.calls(), s(&["down", "up_plain"]));
}

#[tokio::test]
async fn after_the_backoff_exactly_one_build_runs() {
    let d = tempfile::tempdir().unwrap();
    record_attempt(d.path(), &tag(), NOW - 3601).unwrap();
    let f = Fake::new(d.path());
    assert_eq!(run(&f).await.unwrap(), Outcome::Upgraded);
    assert_eq!(f.builds.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_missing_image_builds_even_inside_a_backoff_and_tags_before_stamping() {
    let d = tempfile::tempdir().unwrap();
    record_attempt(d.path(), &tag(), NOW - 10).unwrap();
    let mut f = Fake::new(d.path());
    f.have_image = false;
    assert_eq!(run(&f).await.unwrap(), Outcome::BuiltFresh);
    let t = tag();
    assert_eq!(
        f.calls(),
        vec![
            "refresh_dockerfile".to_string(),
            format!("build:{t}"),
            format!("tag_latest:{t}"),
            "down".to_string(),
            "up_plain".to_string()
        ]
    );
    assert_eq!(*f.stamp_at_tag_latest.lock().unwrap(), Some(false));
    assert_eq!(read_stamp(d.path()), Some(stamp_for(&key_fp(), &pin())));
    assert!(read_backoff(d.path()).is_none());
}

#[tokio::test]
async fn a_failed_fresh_build_leaves_no_stamp() {
    let d = tempfile::tempdir().unwrap();
    let mut f = Fake::new(d.path());
    f.have_image = false;
    f.build_ok = false;
    assert!(run(&f).await.is_err());
    assert!(read_stamp(d.path()).is_none());
    assert!(!f.calls().iter().any(|x| x.starts_with("tag_latest")));
}

#[test]
fn install_tags_latest_before_writing_the_stamp() {
    let d = tempfile::tempdir().unwrap();
    let f = Fake::new(d.path());
    after_fresh_build(&f, d.path(), &tag(), &key_fp(), &pin()).unwrap();
    assert_eq!(f.calls(), vec![format!("tag_latest:{}", tag())]);
    assert_eq!(*f.stamp_at_tag_latest.lock().unwrap(), Some(false));
    assert_eq!(read_stamp(d.path()), Some(stamp_for(&key_fp(), &pin())));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_starts_run_exactly_one_build() {
    let d = tempfile::tempdir().unwrap();
    let f = Arc::new(Fake::new(d.path()));
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let f = Arc::clone(&f);
            tokio::spawn(async move { run(&f).await.unwrap() })
        })
        .collect();
    let mut outs = Vec::new();
    for h in handles {
        outs.push(h.await.unwrap());
    }
    assert_eq!(f.builds.load(Ordering::SeqCst), 1);
    assert_eq!(outs.iter().filter(|o| **o == Outcome::Upgraded).count(), 1);
    assert_eq!(outs.iter().filter(|o| **o == Outcome::Ran).count(), 9);
}

// --------------------------------------------------------------- redaction

#[test]
fn redaction_masks_every_planted_form() {
    let raw = key();
    let bearer = "nk-synthetic-bearer-value";
    let embedded = format!("sk-fem-{}", "cd".repeat(32));
    let text = format!(
        "ERR build arg {raw} failed\nAPI_KEY={bearer}\nx{embedded}y and again {embedded}\n(\"{raw}\"), key:{bearer}, [{raw}]"
    );
    let out = redact_node_output(&text, &[&raw, bearer]);
    assert!(!out.contains(&raw), "{out}");
    assert!(!out.contains(bearer), "{out}");
    assert!(!out.contains(&embedded), "{out}");
    assert!(!out.contains("abababab"), "{out}");
    assert!(out.contains(&format!("[diiisco-key fp={}]", fp12(&raw))));
    assert!(out.contains(&format!("[diiisco-key fp={}]", fp12(bearer))));
    assert!(out.contains(&format!("[diiisco-key fp={}]", fp12(&embedded))));
    assert!(out.contains("ERR build arg"));
    assert_eq!(out.matches("[diiisco-key fp=").count(), 7);
}

#[test]
fn redaction_leaves_text_without_keys_unchanged() {
    let text = "Step 4/9 : RUN npm install\nsk-fem- is only a prefix\nsk-fem-abc";
    assert_eq!(redact_node_output(text, &[]), text);
    assert_eq!(redact_node_output(text, &[""]), text);
}

// ------------------------------------------- F1: Dockerfile on disk is current

#[test]
fn writer_replaces_a_stale_dockerfile_with_the_embedded_one() {
    let d = tempfile::tempdir().unwrap();
    let node = d.path().join("diiisco-node");
    std::fs::create_dir_all(&node).unwrap();
    std::fs::write(
        node.join("Dockerfile"),
        "FROM stale\nRUN git clone --depth 1 x .\n",
    )
    .unwrap();
    let wrote = write_embedded_dockerfile(d.path());
    assert!(wrote.is_ok(), "writer must succeed");
    let got = std::fs::read_to_string(node.join("Dockerfile"));
    assert!(got.is_ok(), "Dockerfile readable after the write");
    assert!(
        got.unwrap() == DOCKERFILE,
        "on-disk Dockerfile must equal the embedded one"
    );
    let names: Vec<String> = std::fs::read_dir(&node)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["Dockerfile".to_string()], "no tmp file left");
}

#[test]
fn writer_creates_the_node_dir_when_absent() {
    let d = tempfile::tempdir().unwrap();
    let wrote = write_embedded_dockerfile(d.path());
    assert!(wrote.is_ok());
    let got = std::fs::read_to_string(d.path().join("diiisco-node").join("Dockerfile"));
    assert!(got.is_ok_and(|g| g == DOCKERFILE));
}

#[tokio::test]
async fn the_dockerfile_is_refreshed_before_every_build() {
    for have_image in [true, false] {
        let d = tempfile::tempdir().unwrap();
        let mut f = Fake::new(d.path());
        f.have_image = have_image;
        run(&f).await.unwrap();
        let c = f.calls();
        let refresh = c.iter().position(|x| x == "refresh_dockerfile");
        let build = c.iter().position(|x| x.starts_with("build:"));
        assert!(refresh.is_some() && build.is_some(), "{c:?}");
        assert!(refresh < build, "refresh must precede the build: {c:?}");
    }
}

#[tokio::test]
async fn a_failed_refresh_builds_nothing_and_keeps_the_old_node() {
    let d = tempfile::tempdir().unwrap();
    let mut f = Fake::new(d.path());
    f.refresh_ok = false;
    let out = run(&f).await.unwrap();
    assert_eq!(out, Outcome::UpgradeFailed);
    assert_eq!(f.builds.load(Ordering::SeqCst), 0);
    assert_eq!(
        f.calls().last().map(String::as_str),
        Some("up_no_build:none")
    );
    assert!(read_stamp(d.path()).is_none());
    // fresh path: the error propagates
    let d2 = tempfile::tempdir().unwrap();
    let mut f2 = Fake::new(d2.path());
    f2.have_image = false;
    f2.refresh_ok = false;
    assert!(run(&f2).await.is_err());
    assert_eq!(f2.builds.load(Ordering::SeqCst), 0);
}

#[test]
fn start_and_the_build_site_refresh_the_dockerfile() {
    let body = non_test(include_str!("diiisco.rs"));
    assert_eq!(body.matches("write_embedded_dockerfile(").count(), 2);
    let start = body.split("async fn start(").nth(1).unwrap_or("");
    let start = start.split("async fn stop(").next().unwrap_or("");
    assert!(
        start.contains("write_embedded_dockerfile("),
        "start refreshes beside the compose"
    );
    assert!(body.contains("fn refresh_dockerfile(&self)"));
}

#[test]
fn dockerfile_no_longer_mentions_the_ts_node_fallback() {
    assert!(!DOCKERFILE.contains("ts-node"));
}

// ------------------------------------------- F2: clock read after the lock

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_clock_is_read_only_after_the_lock_is_held() {
    let d = tempfile::tempdir().unwrap();
    let f = Arc::new(Fake::new(d.path()));
    let reads = Arc::new(AtomicUsize::new(0));
    let guard = REBUILD_LOCK.lock().await;
    let (f2, r2) = (Arc::clone(&f), Arc::clone(&reads));
    let h = tokio::spawn(async move {
        run_node_sequence(&*f2, &f2.dir, &key_fp(), &pin(), move || {
            r2.fetch_add(1, Ordering::SeqCst);
            NOW
        })
        .await
        .unwrap()
    });
    for _ in 0..20 {
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(
        reads.load(Ordering::SeqCst),
        0,
        "no clock read while queued"
    );
    drop(guard);
    h.await.unwrap();
    assert_eq!(
        reads.load(Ordering::SeqCst),
        1,
        "read exactly once, after the lock"
    );
}

#[tokio::test]
async fn a_waiter_planning_after_the_winner_sees_the_recorded_attempt() {
    // The winner recorded an attempt at T. A waiter whose clock is read after
    // the lock (>= T) defers; read before it (T-1) it would see a future
    // attempt, ignore the backoff and build again.
    let d = tempfile::tempdir().unwrap();
    record_attempt(d.path(), &tag(), NOW).unwrap();
    let f = Fake::new(d.path());
    let out = run_node_sequence(&f, &f.dir, &key_fp(), &pin(), || NOW + 1).await;
    assert!(out.is_ok_and(|o| o == Outcome::Ran));
    assert_eq!(f.builds.load(Ordering::SeqCst), 0);
}

// ----------------------------------------------- F5: mnemonic is masked

#[test]
fn redaction_masks_a_planted_mnemonic_and_the_call_sites_pass_it() {
    let mnemonic = "abandon ability able about above absent absorb abstract";
    let text = format!("ERROR: bad phrase [{mnemonic}] and again {mnemonic}.");
    let out = redact_node_output(&text, &[&key(), mnemonic]);
    assert!(!out.contains("abandon ability"), "{out}");
    assert!(!out.contains("abstract"), "{out}");
    assert_eq!(out.matches("[diiisco-key fp=").count(), 2);
    let body = non_test(include_str!("diiisco.rs"));
    for (i, _) in body.match_indices("redact_out(") {
        let w = &body[i..(i + 200).min(body.len())];
        if w.starts_with("redact_out(s: &str") {
            continue;
        }
        assert!(
            w.contains("mnemonic"),
            "redact_out call at byte {i} omits the mnemonic"
        );
    }
    assert!(body.matches("redact_out(").count() >= 3);
}

// ---------------------------------------------------------- static contents

fn hex40_count(text: &str) -> usize {
    let re = regex::Regex::new("[0-9a-f]{40}").unwrap();
    re.find_iter(text).count()
}

#[test]
fn dockerfile_has_exactly_one_pin_on_the_arg_line_and_checks_it_out() {
    assert_eq!(hex40_count(DOCKERFILE), 1);
    let arg: Vec<&str> = DOCKERFILE
        .lines()
        .filter(|l| l.starts_with("ARG DIIISCO_NODE_SHA="))
        .collect();
    assert_eq!(arg.len(), 1);
    assert_eq!(hex40_count(arg[0]), 1);
    assert!(DOCKERFILE.contains("git fetch --depth 1 origin \"$DIIISCO_NODE_SHA\""));
    assert!(DOCKERFILE.contains("git checkout --detach FETCH_HEAD"));
    assert!(DOCKERFILE.contains("test \"$(git rev-parse HEAD)\" = \"$DIIISCO_NODE_SHA\""));
    assert!(
        !DOCKERFILE.contains("git clone"),
        "no unpinned clone of upstream HEAD"
    );
}

#[test]
fn pin_parsed_from_the_embedded_dockerfile_equals_the_arg_value() {
    let want = DOCKERFILE
        .lines()
        .find_map(|l| l.strip_prefix("ARG DIIISCO_NODE_SHA="));
    assert!(want.is_some(), "Dockerfile carries the pin ARG");
    assert_eq!(pin_sha().as_deref(), want);
    assert_eq!(pin_sha_of("ARG DIIISCO_NODE_SHA=nothex"), None);
    assert_eq!(pin_sha_of("ARG DIIISCO_NODE_SHA="), None);
}

#[test]
fn dockerfile_binds_port_8181_on_all_interfaces_and_asserts_it() {
    assert!(DOCKERFILE.contains("s#^    port: 8080,#    port: 8181,\\n    host: \"0.0.0.0\",#"));
    assert!(DOCKERFILE.contains("grep -q '^    port: 8181,$' src/environment/environment.ts"));
    assert!(
        DOCKERFILE.contains("grep -q '^    host: \"0.0.0.0\",$' src/environment/environment.ts")
    );
    assert!(DOCKERFILE.contains("test -f dist/dev.js"));
}

#[test]
fn dockerfile_cmd_starts_dev_js_without_any_runtime_fetch() {
    let cmd: Vec<&str> = DOCKERFILE
        .lines()
        .filter(|l| l.trim_start().starts_with("CMD "))
        .collect();
    assert_eq!(cmd, vec!["CMD [\"node\", \"dist/dev.js\"]"]);
    assert!(!cmd[0].contains("npx"));
    assert!(!cmd[0].contains("dist/index.js"));
}

#[test]
fn compose_healthcheck_uses_unauthenticated_health_and_the_tagged_image() {
    assert!(COMPOSE.contains("curl -sf http://localhost:8181/health || exit 1"));
    assert!(!COMPOSE.contains("/v1/models"));
    assert!(!COMPOSE.contains("sk-"), "no key literal in the compose");
    assert!(COMPOSE.contains("image: diiisco-node:${DIIISCO_IMAGE_TAG:-latest}"));
    assert!(COMPOSE.contains("\"127.0.0.1:8181:8181\""));
}

// ------------------------------------------------------- source structure

fn non_test(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap()
}

#[test]
fn diiisco_rs_keeps_its_frozen_literals_and_has_no_cfg_test_token() {
    let body = non_test(include_str!("diiisco.rs"));
    assert!(!body.contains("cfg(test)"), "A1: no cfg(test) token");
    assert_eq!(body.matches("\"compose\", \"build\"").count(), 2);
    assert_eq!(body.matches("require_diiisco_bearer(&creds)?").count(), 2);
    assert!(body.contains("#[path = \"diiisco_rebuild.rs\"]"));
    assert!(!include_str!("diiisco_rebuild.rs").contains("cfg(test)"));
}

#[test]
fn every_compose_invocation_sets_or_removes_the_image_tag_env() {
    for (name, src) in [
        ("diiisco.rs", non_test(include_str!("diiisco.rs"))),
        ("diiisco_rebuild.rs", include_str!("diiisco_rebuild.rs")),
    ] {
        for (i, _) in src.match_indices("\"compose\"") {
            let lo = src[..i].char_indices().rev().nth(300).map_or(0, |(p, _)| p);
            let window = &src[lo..(i + 300).min(src.len())];
            assert!(
                window.contains("DIIISCO_IMAGE_TAG") || window.contains("node_cmd("),
                "{name}: compose invocation at byte {i} neither sets nor removes the tag env"
            );
        }
    }
}

#[test]
fn stop_bumps_the_generation_before_it_stops_compose_and_start_captures_it() {
    let body = non_test(include_str!("diiisco.rs"));
    let stop = body.split("async fn stop(").nth(1).unwrap();
    let stop = stop.split("async fn health_check").next().unwrap();
    let bump = stop.find("bump_stop_generation()");
    let compose = stop.find("\"compose\"");
    assert!(bump.is_some(), "stop bumps the generation");
    assert!(compose.is_some(), "stop runs compose");
    assert!(bump < compose);
    let start = body.split("async fn start(").nth(1).unwrap();
    let start = start.split("async fn stop(").next().unwrap();
    assert!(start.contains("stop_generation()"));
    assert!(
        start.contains("tokio::spawn("),
        "sequence must outlive a dropped start()"
    );
}

#[test]
fn start_gate_is_image_missing_or_plan_build_and_attempt_is_recorded_first() {
    let body = non_test(include_str!("diiisco.rs"));
    assert_eq!(
        body.matches("diiisco_rebuild::run_node_sequence(").count(),
        1
    );
    assert_eq!(
        body.matches("diiisco_rebuild::after_fresh_build(").count(),
        1
    );
    let seq = non_test(include_str!("diiisco_rebuild.rs"));
    assert_eq!(seq.matches("let must_build = !have_image ||").count(), 1);
    let upgrade = seq.split("// Upgrade:").nth(1).unwrap_or("");
    let rec = upgrade.find("record_attempt(dir");
    let build = upgrade.find("ops.build(");
    assert!(
        rec.is_some() && build.is_some(),
        "upgrade records then builds"
    );
    assert!(rec < build, "B4: record the attempt BEFORE the build");
}
