//! One-time gated rebuild tests. Synthetic values only; key values are never
//! printed in assertion messages. The Docker side is a recording fake.
use super::super::diiisco_rebuild::{
    after_fresh_build, clear_backoff, delay_secs, fp12, gate_plan, image_tag, pin_for_tag, pin_sha,
    pin_sha_of, plan_start, probe_verdict, read_backoff, read_stamp, record_attempt,
    redact_node_output, rename_retrying, run_node_sequence, write_embedded_dockerfile, write_stamp,
    Backoff, NodeOps, Outcome, Plan, Stamp, BACKOFF_FILE, REBUILD_ENABLED, REBUILD_LOCK,
    STAMP_FILE, UNPINNED,
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
    assert!(probe_verdict(500, 401), "F2: a post-auth 500 promotes");
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
    run_node_sequence(f, &f.dir, &key_fp(), &pin(), || NOW, true).await
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
        run_node_sequence(
            &*f2,
            &f2.dir,
            &key_fp(),
            &pin(),
            move || {
                r2.fetch_add(1, Ordering::SeqCst);
                NOW
            },
            true,
        )
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
    let out = run_node_sequence(&f, &f.dir, &key_fp(), &pin(), || NOW + 1, true).await;
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

// BLOCKER-SANDBOX per prompt line 354, 2026-10-14: the pin assertions below
// hold the pinned form only while the upgrade rebuild is enabled.
#[test]
fn dockerfile_pin_form_matches_the_rebuild_switch() {
    let arg: Vec<&str> = DOCKERFILE
        .lines()
        .filter(|l| l.starts_with("ARG DIIISCO_NODE_SHA="))
        .collect();
    if REBUILD_ENABLED {
        assert_eq!(hex40_count(DOCKERFILE), 1);
        assert_eq!(arg.len(), 1);
        assert_eq!(hex40_count(arg[0]), 1);
        assert!(DOCKERFILE.contains("git fetch --depth 1 origin \"$DIIISCO_NODE_SHA\""));
        assert!(DOCKERFILE.contains("git checkout --detach FETCH_HEAD"));
        assert!(DOCKERFILE.contains("test \"$(git rev-parse HEAD)\" = \"$DIIISCO_NODE_SHA\""));
        assert!(
            !DOCKERFILE.contains("git clone"),
            "no unpinned clone of upstream HEAD"
        );
    } else {
        assert_eq!(
            hex40_count(DOCKERFILE),
            0,
            "no pin while the rebuild is disabled"
        );
        assert!(arg.is_empty(), "no pin ARG while the rebuild is disabled");
        let clones: Vec<&str> = DOCKERFILE
            .lines()
            .filter(|l| l.starts_with("RUN git clone"))
            .collect();
        assert_eq!(
            clones,
            vec!["RUN git clone --depth 1 https://github.com/Diiisco-Inc/diiisco-node.git ."]
        );
    }
}

#[test]
fn pin_parsed_from_the_embedded_dockerfile_equals_the_arg_value() {
    let want = DOCKERFILE
        .lines()
        .find_map(|l| l.strip_prefix("ARG DIIISCO_NODE_SHA="));
    assert_eq!(
        pin_sha().as_deref(),
        want,
        "pin_sha() is the ARG value, or None without it"
    );
    assert_eq!(pin_sha_of("ARG DIIISCO_NODE_SHA=nothex"), None);
    assert_eq!(pin_sha_of("ARG DIIISCO_NODE_SHA="), None);
}

#[test]
fn rebuild_is_enabled_and_pinned() {
    let enabled = std::hint::black_box(REBUILD_ENABLED);
    assert!(enabled, "the upgrade rebuild is enabled");
    let pin = pin_sha();
    assert!(
        pin.as_deref().is_some_and(
            |p| p.len() == 40 && p.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        ),
        "an enabled rebuild needs a 40-hex pin in the Dockerfile"
    );
}

#[test]
fn rebuild_enabled_implies_a_pin() {
    assert!(!REBUILD_ENABLED || pin_sha().is_some());
}

#[test]
fn gate_plan_never_rebuilds_an_existing_image_when_disabled() {
    let build = Plan::BuildNew { tag: tag() };
    assert_eq!(gate_plan(build.clone(), false, true), Plan::RunExisting);
    assert_eq!(gate_plan(build.clone(), false, false), build);
    assert_eq!(gate_plan(build.clone(), true, true), build);
    for p in [Plan::RunExisting, Plan::RunOldDeferred] {
        assert_eq!(gate_plan(p.clone(), false, true), p);
        assert_eq!(gate_plan(p.clone(), true, true), p);
    }
}

async fn run_disabled(f: &Fake) -> Result<Outcome> {
    run_node_sequence(f, &f.dir, &key_fp(), &pin(), || NOW, false).await
}

#[tokio::test]
async fn disabled_rebuild_runs_a_legacy_image_without_any_build_state() {
    let d = tempfile::tempdir().unwrap();
    let f = Fake::new(d.path());
    assert_eq!(run_disabled(&f).await.unwrap(), Outcome::Ran);
    assert_eq!(f.calls(), s(&["down", "up_plain"]));
    assert_eq!(f.builds.load(Ordering::SeqCst), 0);
    assert!(read_stamp(d.path()).is_none());
    assert!(read_backoff(d.path()).is_none());
}

#[tokio::test]
async fn disabled_rebuild_ignores_a_stale_stamp_and_an_expired_backoff() {
    let d = tempfile::tempdir().unwrap();
    write_stamp(d.path(), &stamp_for(&fp12("another-key"), &pin())).unwrap();
    record_attempt(d.path(), &tag(), NOW - 100_000).unwrap();
    let f = Fake::new(d.path());
    assert_eq!(run_disabled(&f).await.unwrap(), Outcome::Ran);
    assert_eq!(f.calls(), s(&["down", "up_plain"]));
    assert_eq!(f.builds.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn disabled_rebuild_still_builds_when_there_is_no_image() {
    let d = tempfile::tempdir().unwrap();
    let mut f = Fake::new(d.path());
    f.have_image = false;
    assert_eq!(run_disabled(&f).await.unwrap(), Outcome::BuiltFresh);
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
    assert_eq!(read_stamp(d.path()), Some(stamp_for(&key_fp(), &pin())));
}

#[test]
fn the_unpinned_placeholder_is_safe_and_never_a_real_pin() {
    assert!(UNPINNED.bytes().all(|b| b.is_ascii_lowercase()));
    assert!(UNPINNED.len() != 40);
    assert!(UNPINNED != pin());
    assert!(!UNPINNED.bytes().all(|b| b.is_ascii_hexdigit()));
    assert!(image_tag(&key_fp(), UNPINNED) != image_tag(&key_fp(), &pin()));
    let p = pin_for_tag();
    assert!(p == UNPINNED || (p.len() == 40 && p.bytes().all(|b| b.is_ascii_hexdigit())));
}

#[test]
fn an_unpinned_stamp_round_trips_but_other_non_hex_pins_are_rejected() {
    let d = tempfile::tempdir().unwrap();
    let s = stamp_for(&key_fp(), UNPINNED);
    write_stamp(d.path(), &s).unwrap();
    assert_eq!(read_stamp(d.path()), Some(s));
    std::fs::write(
        d.path().join(STAMP_FILE),
        format!("tag={}\nkey_fp={}\npin_sha=notapin\n", tag(), key_fp()),
    )
    .unwrap();
    assert!(read_stamp(d.path()).is_none());
}

#[test]
fn an_unpinned_and_a_pinned_stamp_never_satisfy_each_others_plan() {
    let pinned = stamp_for(&key_fp(), &pin());
    let unpinned = stamp_for(&key_fp(), UNPINNED);
    assert!(matches!(
        plan_start(Some(&pinned), None, &key_fp(), UNPINNED, NOW),
        Plan::BuildNew { .. }
    ));
    assert!(matches!(
        plan_start(Some(&unpinned), None, &key_fp(), &pin(), NOW),
        Plan::BuildNew { .. }
    ));
    assert_eq!(
        plan_start(Some(&unpinned), None, &key_fp(), UNPINNED, NOW),
        Plan::RunExisting
    );
}

#[test]
fn install_and_start_use_the_gated_switch_and_placeholder_pin() {
    let body = non_test(include_str!("diiisco.rs"));
    assert!(body.contains("diiisco_rebuild::REBUILD_ENABLED"));
    assert_eq!(body.matches("diiisco_rebuild::pin_for_tag()").count(), 2);
    assert!(!body.contains("pin_sha().unwrap_or_default()"));
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

// ------------------------------------- C1-RACE: shared temp file, rename retry

fn dir_names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn concurrent_dockerfile_writes_all_succeed() {
    let d = tempfile::tempdir().unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (b, p) = (Arc::clone(&barrier), d.path().to_path_buf());
            std::thread::spawn(move || {
                b.wait();
                let mut errs = Vec::new();
                for _ in 0..50 {
                    if let Err(e) = write_embedded_dockerfile(&p) {
                        errs.push(e.to_string());
                    }
                }
                errs
            })
        })
        .collect();
    let errs: Vec<String> = handles
        .into_iter()
        .flat_map(|h| h.join().unwrap_or_else(|_| vec!["thread panicked".into()]))
        .collect();
    assert!(
        errs.is_empty(),
        "{} writes failed, e.g. {:?}",
        errs.len(),
        errs.first()
    );
    let node = d.path().join("diiisco-node");
    let got = std::fs::read_to_string(node.join("Dockerfile"));
    assert!(
        got.is_ok_and(|g| g == DOCKERFILE),
        "final content is the embedded Dockerfile"
    );
    assert_eq!(
        dir_names(&node),
        vec!["Dockerfile".to_string()],
        "no temp files left"
    );
}

#[test]
fn concurrent_stamp_writes_and_reads_never_fail_or_tear() {
    let d = tempfile::tempdir().unwrap();
    let stamp = stamp_for(&key_fp(), &pin());
    write_stamp(d.path(), &stamp).unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let mut handles = Vec::new();
    for i in 0..8 {
        let (b, p, s) = (Arc::clone(&barrier), d.path().to_path_buf(), stamp.clone());
        handles.push(std::thread::spawn(move || {
            b.wait();
            let mut bad = Vec::new();
            for _ in 0..50 {
                if i % 2 == 0 {
                    if let Err(e) = write_stamp(&p, &s) {
                        bad.push(format!("write: {e}"));
                    }
                } else if read_stamp(&p).as_ref() != Some(&s) {
                    bad.push("read: absent or torn".to_string());
                }
            }
            bad
        }));
    }
    let bad: Vec<String> = handles
        .into_iter()
        .flat_map(|h| h.join().unwrap_or_else(|_| vec!["thread panicked".into()]))
        .collect();
    assert!(
        bad.is_empty(),
        "{} failures, e.g. {:?}",
        bad.len(),
        bad.first()
    );
    assert_eq!(
        dir_names(d.path()),
        vec![STAMP_FILE.to_string()],
        "no temp files left"
    );
}

#[test]
fn a_failed_write_removes_its_own_temp_file() {
    let d = tempfile::tempdir().unwrap();
    // The target name is an existing directory, so the final rename must fail.
    std::fs::create_dir(d.path().join(STAMP_FILE)).unwrap();
    let r = write_stamp(d.path(), &stamp_for(&key_fp(), &pin()));
    assert!(r.is_err(), "renaming a file over a directory must fail");
    assert_eq!(
        dir_names(d.path()),
        vec![STAMP_FILE.to_string()],
        "the failed write must not leave its temp file behind"
    );
}

fn counting_rename(
    fail_with: std::io::ErrorKind,
    fail_times: usize,
) -> (impl FnMut() -> std::io::Result<()>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&calls);
    let f = move || {
        let n = c.fetch_add(1, Ordering::SeqCst);
        if n < fail_times {
            Err(std::io::Error::from(fail_with))
        } else {
            Ok(())
        }
    };
    (f, calls)
}

#[test]
fn rename_retries_a_sharing_violation_until_it_succeeds() {
    let (f, calls) = counting_rename(std::io::ErrorKind::PermissionDenied, 2);
    assert!(rename_retrying(f).is_ok());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[test]
fn rename_gives_up_after_five_attempts_within_half_a_second() {
    let (f, calls) = counting_rename(std::io::ErrorKind::PermissionDenied, usize::MAX);
    let t = std::time::Instant::now();
    let r = rename_retrying(f);
    assert!(r.is_err_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied));
    assert_eq!(calls.load(Ordering::SeqCst), 5);
    assert!(
        t.elapsed() < std::time::Duration::from_millis(500),
        "{:?}",
        t.elapsed()
    );
}

#[test]
fn rename_does_not_retry_other_errors() {
    let (f, calls) = counting_rename(std::io::ErrorKind::NotFound, usize::MAX);
    assert!(rename_retrying(f).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

// ====================================================== 0.4.37 finish (C1)

use super::super::diiisco_rebuild::{
    install_step, require_pin_when_enabled, stamp_is_current, ImageState, InstallStep,
};

const JC_200_SHA512: &str =
    "sha512-yyrnK/mEm6Na3ChbJUWueXdapueW0p380RUyTW87XGb1ww8l8hU0pRrGC3vSWHe9CxrbPHX2fGUOZpNiHR0IIg==";
const JC_201_SHA512: &str =
    "sha512-TuGrLM400c1VN8bQsSOiyDEEmRjIVgFvcSehWC+0JJaV4BGl1vYNxHBPVsN5ADUZIcWp75IN1yf+vCZyDjPt1Q==";

#[test]
fn f2_probe_verdict_table() {
    // (with-key, no-key) -> promote
    let table: [(u16, u16, bool); 10] = [
        (200, 401, true),
        (500, 401, true),
        (404, 401, true), // lead ruling: reachable and not 401/403 promotes
        (401, 401, false),
        (403, 401, false),
        (200, 200, false),
        (500, 200, false),
        (0, 401, false),
        (200, 0, false),
        (0, 0, false),
    ];
    for (with_key, no_key, want) in table {
        assert_eq!(
            probe_verdict(with_key, no_key),
            want,
            "({with_key},{no_key})"
        );
    }
}

/// A recording fake whose image state can be Unknown or flip to Present on build.
struct Sim {
    inner: Fake,
    present: AtomicBool,
    unknown: bool,
}

impl Sim {
    fn new(dir: &Path, present: bool, unknown: bool) -> Self {
        Sim {
            inner: Fake::new(dir),
            present: AtomicBool::new(present),
            unknown,
        }
    }
}

#[async_trait]
impl NodeOps for Sim {
    fn image_exists(&self) -> bool {
        self.present.load(Ordering::SeqCst)
    }
    fn image_state(&self) -> ImageState {
        if self.unknown {
            ImageState::Unknown
        } else if self.image_exists() {
            ImageState::Present
        } else {
            ImageState::Absent
        }
    }
    fn refresh_dockerfile(&self) -> Result<()> {
        self.inner.refresh_dockerfile()
    }
    fn image_id(&self, reference: &str) -> Option<String> {
        self.inner.image_id(reference)
    }
    fn build(&self, t: &str) -> Result<()> {
        self.inner.build(t)?;
        self.present.store(true, Ordering::SeqCst);
        Ok(())
    }
    fn down(&self) {
        self.inner.down()
    }
    fn up_plain(&self) -> Result<()> {
        self.inner.up_plain()
    }
    fn up_no_build(&self, t: Option<&str>) -> Result<()> {
        self.inner.up_no_build(t)
    }
    async fn probe(&self) -> bool {
        self.inner.probe().await
    }
    fn tag_latest(&self, t: &str) -> Result<()> {
        self.inner.tag_latest(t)
    }
    fn rmi_by_id(&self, id: &str) {
        self.inner.rmi_by_id(id)
    }
    fn stop_requested(&self) -> bool {
        self.inner.stop_requested()
    }
}

/// install()'s locked section, driven through the same pure steps it uses.
async fn install_path(sim: &Sim) -> Result<bool> {
    let _g = REBUILD_LOCK.lock().await;
    let (kfp, p, t) = (key_fp(), pin(), tag());
    match install_step(
        sim.image_state(),
        stamp_is_current(&sim.inner.dir, &kfp, &p),
    ) {
        InstallStep::Build => {
            sim.build(&t)?;
            after_fresh_build(sim, &sim.inner.dir, &t, &kfp, &p)?;
            Ok(true)
        }
        InstallStep::AlreadyBuilt => Ok(false),
        InstallStep::Unknown => anyhow::bail!("image state unknown"),
    }
}

fn file_bytes(dir: &Path, name: &str) -> Option<Vec<u8>> {
    std::fs::read(dir.join(name)).ok()
}

#[tokio::test]
async fn unknown_image_state_builds_nothing_and_leaves_state_files_identical() {
    // stale stamp + a recorded backoff, both must survive byte for byte
    let d = tempfile::tempdir().unwrap();
    write_stamp(d.path(), &stamp_for(&fp12("another-key"), &pin())).unwrap();
    record_attempt(d.path(), &tag(), NOW - 10).unwrap();
    let (stamp0, backoff0) = (
        file_bytes(d.path(), STAMP_FILE),
        file_bytes(d.path(), BACKOFF_FILE),
    );
    assert!(stamp0.is_some() && backoff0.is_some());
    let sim = Sim::new(d.path(), true, true);
    let out = run_node_sequence(&sim, d.path(), &key_fp(), &pin(), || NOW + 5000, true).await;
    assert_eq!(format!("{out:?}"), "Ok(ImageUnknown)");
    assert!(sim.inner.calls().is_empty(), "{:?}", sim.inner.calls());
    assert_eq!(sim.inner.builds.load(Ordering::SeqCst), 0);
    assert_eq!(file_bytes(d.path(), STAMP_FILE), stamp0);
    assert_eq!(file_bytes(d.path(), BACKOFF_FILE), backoff0);
    // no state at all: nothing is created either
    let d2 = tempfile::tempdir().unwrap();
    let sim2 = Sim::new(d2.path(), false, true);
    let _ = run_node_sequence(&sim2, d2.path(), &key_fp(), &pin(), || NOW, true).await;
    assert!(sim2.inner.calls().is_empty(), "{:?}", sim2.inner.calls());
    assert!(
        dir_names(d2.path()).is_empty(),
        "{:?}",
        dir_names(d2.path())
    );
}

#[test]
fn install_step_table() {
    use ImageState::{Absent, Present, Unknown};
    assert_eq!(install_step(Present, true), InstallStep::AlreadyBuilt);
    assert_eq!(install_step(Present, false), InstallStep::Build);
    assert_eq!(install_step(Absent, true), InstallStep::Build);
    assert_eq!(install_step(Absent, false), InstallStep::Build);
    assert_eq!(install_step(Unknown, true), InstallStep::Unknown);
    assert_eq!(install_step(Unknown, false), InstallStep::Unknown);
}

#[test]
fn install_checks_inside_the_lock_before_building_and_maps_unknown() {
    let body = non_test(include_str!("diiisco.rs"));
    let install = body.split("async fn install(&self)").nth(1).unwrap();
    let install = install.split("async fn start(").next().unwrap();
    let lock = install.find("REBUILD_LOCK.lock()").unwrap();
    let step = install.find("install_step(").unwrap();
    let build = install.find("\"compose\", \"build\"").unwrap();
    assert!(
        lock < step && step < build,
        "re-check sits between lock and build"
    );
    assert!(install.contains("already built by a concurrent caller"));
    let iv = body.split("fn installed_version(").nth(1).unwrap();
    let iv = iv.split("fn collect_poc_data").next().unwrap();
    assert!(
        iv.contains("ImageState::Absent"),
        "only Absent means not installed"
    );
    assert!(!iv.contains("!image_built()"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_install_paths_and_start_build_exactly_once() {
    let d = tempfile::tempdir().unwrap();
    let sim = Arc::new(Sim::new(d.path(), false, false));
    let work = async {
        let (a, b) = (Arc::clone(&sim), Arc::clone(&sim));
        let i1 = tokio::spawn(async move { install_path(&a).await.unwrap() });
        let i2 = tokio::spawn(async move { install_path(&b).await.unwrap() });
        let c = Arc::clone(&sim);
        let st = tokio::spawn(async move {
            run_node_sequence(&*c, &c.inner.dir, &key_fp(), &pin(), || NOW, true)
                .await
                .unwrap()
        });
        let _ = (i1.await.unwrap(), i2.await.unwrap(), st.await.unwrap());
    };
    tokio::time::timeout(std::time::Duration::from_secs(20), work)
        .await
        .expect("no deadlock");
    let calls = sim.inner.calls();
    assert_eq!(sim.inner.builds.load(Ordering::SeqCst), 1, "{calls:?}");
    let latest = format!("tag_latest:{}", tag());
    assert_eq!(
        calls.iter().filter(|c| **c == latest).count(),
        1,
        "{calls:?}"
    );
    assert!(!calls.iter().any(|c| c.starts_with("rmi:")), "{calls:?}");
    assert!(stamp_is_current(d.path(), &key_fp(), &pin()));
}

#[tokio::test]
async fn nested_start_then_install_path_completes_without_deadlock() {
    let d = tempfile::tempdir().unwrap();
    let sim = Sim::new(d.path(), false, false);
    let work = async {
        let out = run_node_sequence(&sim, d.path(), &key_fp(), &pin(), || NOW, true)
            .await
            .unwrap();
        let built = install_path(&sim).await.unwrap();
        (out, built)
    };
    let (out, built) = tokio::time::timeout(std::time::Duration::from_secs(10), work)
        .await
        .expect("deadlock: the lock is not re-entrant");
    assert_eq!(out, Outcome::BuiltFresh);
    assert!(!built, "install path must see the image and the stamp");
    assert_eq!(sim.inner.builds.load(Ordering::SeqCst), 1);
    // exactly two lockers (sequence + install), none inside the NodeOps impl
    let seq = non_test(include_str!("diiisco_rebuild.rs"));
    let inst = non_test(include_str!("diiisco.rs"));
    assert_eq!(seq.matches("REBUILD_LOCK.lock()").count(), 1);
    assert_eq!(inst.matches("REBUILD_LOCK.lock()").count(), 1);
}

#[tokio::test]
async fn an_enabled_rebuild_without_a_pin_is_an_error_and_builds_nothing() {
    assert!(require_pin_when_enabled(true, UNPINNED).is_err());
    assert!(require_pin_when_enabled(true, &pin()).is_ok());
    assert!(require_pin_when_enabled(false, UNPINNED).is_ok());
    let d = tempfile::tempdir().unwrap();
    let f = Fake::new(d.path());
    let r = run_node_sequence(&f, d.path(), &key_fp(), UNPINNED, || NOW, true).await;
    assert!(r.is_err(), "enabled + unpinned must refuse");
    assert!(f.calls().is_empty(), "{:?}", f.calls());
    assert_eq!(f.builds.load(Ordering::SeqCst), 0);
    assert!(dir_names(d.path()).is_empty());
}

#[test]
fn dockerfile_restores_the_pinned_fetch() {
    assert_eq!(hex40_count(DOCKERFILE), 1);
    assert!(pin_sha().is_some());
    assert!(DOCKERFILE.contains("git fetch --depth 1 origin \"$DIIISCO_NODE_SHA\""));
    assert!(DOCKERFILE.contains("git checkout --detach FETCH_HEAD"));
    assert!(DOCKERFILE.contains("test \"$(git rev-parse HEAD)\" = \"$DIIISCO_NODE_SHA\""));
    assert!(!DOCKERFILE.contains("git clone"));
}

#[test]
fn dockerfile_pins_json_canonicalize_to_2_0_0_with_four_build_assertions() {
    assert!(DOCKERFILE.contains(
        "npm pkg set dependencies.json-canonicalize=2.0.0 overrides.json-canonicalize=2.0.0"
    ));
    assert!(DOCKERFILE.contains("npm install --save-exact json-canonicalize@2.0.0"));
    assert!(!DOCKERFILE.contains("npm install --save json-canonicalize"));
    // (1) installed version
    assert!(DOCKERFILE.contains(
        r#"require('./node_modules/json-canonicalize/package.json').version")" = "2.0.0""#
    ));
    // (2) the bundle the package main points at
    assert!(DOCKERFILE.contains("test -f node_modules/json-canonicalize/bundles/index.umd.js"));
    // (3) every lock entry's integrity
    assert!(DOCKERFILE.contains("package-lock.json"));
    assert!(DOCKERFILE.contains("node_modules/json-canonicalize"));
    assert!(DOCKERFILE.contains(JC_200_SHA512));
    assert!(
        !DOCKERFILE.contains(JC_201_SHA512),
        "the broken 2.0.1 integrity"
    );
    // (4) npm ls: errors fail the chain, 2.0.0 present, 2.0.1 absent
    assert!(DOCKERFILE.contains("npm ls json-canonicalize --all"));
    assert!(DOCKERFILE.contains("grep -q 'json-canonicalize@2\\.0\\.0'"));
    assert!(DOCKERFILE.contains("! grep -q 'json-canonicalize@2\\.0\\.1'"));
}

#[test]
fn dockerfile_patches_the_dev_js_environment_import_with_a_fallback() {
    let old = r#"[".", "environment", "environment"].join("/")"#;
    let new = r#""./environment/environment.js""#;
    assert!(DOCKERFILE.contains(old));
    assert!(DOCKERFILE.contains(new));
    assert!(DOCKERFILE.contains(
        r##"sed -i 's#\[".", "environment", "environment"\]\.join("/")#"./environment/environment.js"#' dist/dev.js"##
    ));
    assert!(DOCKERFILE
        .contains(r#"! grep -qF '[".", "environment", "environment"].join("/")' dist/dev.js"#));
    assert!(DOCKERFILE.contains("cp dist/environment/environment.js dist/environment/environment"));
    assert!(DOCKERFILE.contains("cmp dist/environment/environment.js dist/environment/environment"));
    assert!(DOCKERFILE.contains("if [ ! -f dist/environment/environment.js ]"));
    assert!(DOCKERFILE.contains("FATAL"));
    let dev = DOCKERFILE.find("RUN test -f dist/dev.js").unwrap();
    let patch = DOCKERFILE.find("fallback copy applied").unwrap();
    assert!(dev < patch, "patch runs after the dev.js existence check");
}

#[test]
fn dockerfile_keeps_port_sed_and_the_cmd() {
    assert!(DOCKERFILE.contains("s#^    port: 8080,#    port: 8181,\\n    host: \"0.0.0.0\",#"));
    assert!(DOCKERFILE.contains("grep -q '^    port: 8181,$' src/environment/environment.ts"));
    assert!(DOCKERFILE.contains("RUN test -f dist/dev.js"));
    assert!(DOCKERFILE.contains("CMD [\"node\", \"dist/dev.js\"]"));
    assert!(COMPOSE.contains("curl -sf http://localhost:8181/health || exit 1"));
}
