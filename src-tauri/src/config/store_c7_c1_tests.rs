//! D-C7-1: the BL-C6-3 sweep must never take a temp a live writer may still
//! be writing, and must also clear what v0.4.33 left behind.
//!
//! - Only a temp at least 60 s old goes (age-only rule, A7-P2); a fresh one,
//!   a future mtime and an unreadable mtime all keep the file.
//! - v0.4.33's fixed `<name>.tmp` goes under the same rule, next to the
//!   primary, the backup and the roaming copy; nothing else is touched.
//! - Every save sweeps too, before it writes.
//! - A delete that fails is one warn line per sweep, and never fails a load or
//!   a save.

use super::*;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fem_c7c1_{tag}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const CONFIG: &str = r#"{"miner_key":"FEM-C7C1-TEST-KEY","wallet_address":"W"}"#;

fn set_mtime(path: &Path, mtime: SystemTime) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(mtime).unwrap();
}

fn aged() -> SystemTime {
    SystemTime::now() - Duration::from_secs(120)
}

/// A temp another (killed) process left next to `target`, `mtime` old.
fn foreign_temp(target: &Path, n: u32, mtime: SystemTime) -> PathBuf {
    let name = target.file_name().unwrap().to_string_lossy().to_string();
    let pid = std::process::id().wrapping_add(1 + n);
    let tmp = target.with_file_name(format!("{name}.tmp.{pid}.1790694283162692900.{n}"));
    std::fs::write(&tmp, CONFIG).unwrap();
    set_mtime(&tmp, mtime);
    tmp
}

/// v0.4.33's fixed temp name for `target`: `path.with_extension("json.tmp")`.
fn legacy_temp(target: &Path, mtime: SystemTime) -> PathBuf {
    let tmp = target.with_extension("json.tmp");
    std::fs::write(&tmp, CONFIG).unwrap();
    set_mtime(&tmp, mtime);
    tmp
}

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Runs `f` under a subscriber local to this thread and returns the WARN
/// lines it logged.
fn warn_lines<T>(f: impl FnOnce() -> T) -> (T, Vec<String>) {
    let captured = Captured::default();
    let sink = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || sink.clone())
        .with_max_level(tracing::Level::WARN)
        .with_ansi(false)
        .finish();
    let out = tracing::subscriber::with_default(subscriber, f);
    let text = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    let lines = text
        .lines()
        .filter(|l| l.contains("WARN"))
        .map(str::to_string)
        .collect();
    (out, lines)
}

#[test]
fn a_fresh_foreign_temp_survives_load() {
    let dir = unique_dir("fresh_load");
    let primary = dir.join("fem_config.json");
    std::fs::write(&primary, CONFIG).unwrap();
    let fresh = foreign_temp(&primary, 0, SystemTime::now());
    let aged = foreign_temp(&primary, 1, aged());

    let _store = ConfigStore::new(dir.clone(), None);
    assert!(!aged.exists(), "control: the sweep ran");
    assert!(
        fresh.exists(),
        "a temp under 60 s old may be a live writer's"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_aged_foreign_temp_is_removed_at_load() {
    let dir = unique_dir("aged_load");
    let roaming_dir = unique_dir("aged_load_roaming");
    let primary = dir.join("fem_config.json");
    let roaming = roaming_dir.join("fem_config.json");
    std::fs::write(&primary, CONFIG).unwrap();
    let temps = [
        foreign_temp(&primary, 0, aged()),
        foreign_temp(&dir.join("fem_config.backup.json"), 1, aged()),
        foreign_temp(&roaming, 2, aged()),
    ];

    let _store = ConfigStore::new(dir.clone(), Some(roaming));
    for t in &temps {
        assert!(!t.exists(), "{} survived the load", t.display());
    }
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&roaming_dir);
}

/// A clock stepped back makes every mtime look like the future: keep it.
#[test]
fn a_temp_with_a_future_mtime_is_kept() {
    let dir = unique_dir("future");
    let primary = dir.join("fem_config.json");
    std::fs::write(&primary, CONFIG).unwrap();
    let future = foreign_temp(&primary, 0, SystemTime::now() + Duration::from_secs(120));
    let aged = foreign_temp(&primary, 1, aged());

    let _store = ConfigStore::new(dir.clone(), None);
    assert!(!aged.exists(), "control: the sweep ran");
    assert!(future.exists(), "a future mtime was treated as old");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_aged_foreign_temp_is_swept_before_a_save_and_a_fresh_one_survives() {
    let dir = unique_dir("save");
    let roaming_dir = unique_dir("save_roaming");
    let primary = dir.join("fem_config.json");
    let backup = dir.join("fem_config.backup.json");
    let roaming = roaming_dir.join("fem_config.json");
    std::fs::write(&primary, CONFIG).unwrap();
    let store = ConfigStore::new(dir.clone(), Some(roaming.clone()));
    // Planted after the load, so only the save can remove them.
    let aged_temps = [
        foreign_temp(&primary, 0, aged()),
        foreign_temp(&backup, 1, aged()),
        foreign_temp(&roaming, 2, aged()),
        legacy_temp(&primary, aged()),
        legacy_temp(&backup, aged()),
        legacy_temp(&roaming, aged()),
    ];
    let fresh = [
        foreign_temp(&primary, 3, SystemTime::now()),
        foreign_temp(&backup, 4, SystemTime::now()),
        foreign_temp(&roaming, 5, SystemTime::now()),
    ];

    store
        .update(|c| c.wallet_address = Some("W2".to_string()))
        .unwrap();
    for t in &aged_temps {
        assert!(!t.exists(), "{} survived the save", t.display());
    }
    for t in &fresh {
        assert!(t.exists(), "{} was removed by the save", t.display());
    }
    assert_eq!(
        ConfigStore::new(dir.clone(), None)
            .get()
            .wallet_address
            .as_deref(),
        Some("W2"),
        "the save itself went through"
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&roaming_dir);
}

/// v0.4.33 wrote `fem_config.json.tmp`, `fem_config.backup.json.tmp` and the
/// roaming `fem_config.json.tmp`. Those exact names go once aged; the real
/// files beside them, and names that only look alike, never do.
#[test]
fn aged_v0433_temps_are_removed_and_nothing_else_is_touched() {
    let dir = unique_dir("legacy");
    let roaming_dir = unique_dir("legacy_roaming");
    let primary = dir.join("fem_config.json");
    let backup = dir.join("fem_config.backup.json");
    let roaming = roaming_dir.join("fem_config.json");
    let legacy = [
        legacy_temp(&primary, aged()),
        legacy_temp(&backup, aged()),
        legacy_temp(&roaming, aged()),
    ];
    assert_eq!(
        legacy
            .iter()
            .map(|p| p.file_name().unwrap())
            .collect::<Vec<_>>(),
        [
            "fem_config.json.tmp",
            "fem_config.backup.json.tmp",
            "fem_config.json.tmp"
        ],
        "control: the v0.4.33 names"
    );
    let real = [
        (primary.clone(), CONFIG.to_string()),
        (backup.clone(), CONFIG.replace("W\"", "WB\"")),
        (roaming.clone(), CONFIG.replace("W\"", "WR\"")),
        (
            dir.join("fem_config.corrupt.1790694283.json"),
            "{".to_string(),
        ),
        (
            dir.join("fem_config.backup.corrupt.1790694283.json"),
            "[".to_string(),
        ),
        (dir.join("update-state.json"), "{}".to_string()),
    ];
    let lookalikes = [
        dir.join("fem_config.tmp"),
        dir.join("fem_config.json.tmpx"),
        dir.join("fem_config.json.tmp.bak"),
        dir.join("xfem_config.json.tmp"),
        dir.join("other.json.tmp"),
        dir.join("update-state.json.tmp"),
        roaming_dir.join("fem_config.backup.json.tmp"),
    ];
    for (p, body) in &real {
        std::fs::write(p, body).unwrap();
        set_mtime(p, aged());
    }
    for p in &lookalikes {
        std::fs::write(p, CONFIG).unwrap();
        set_mtime(p, aged());
    }

    let _store = ConfigStore::new(dir.clone(), Some(roaming.clone()));
    for p in &legacy {
        assert!(!p.exists(), "{} survived the load", p.display());
    }
    for (p, body) in &real {
        assert_eq!(
            std::fs::read_to_string(p).ok().as_deref(),
            Some(body.as_str()),
            "{} was touched",
            p.display()
        );
    }
    for p in &lookalikes {
        assert!(p.exists(), "{} was removed", p.display());
    }
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&roaming_dir);
}

#[test]
fn fresh_v0433_temps_survive_load_and_save() {
    let dir = unique_dir("legacy_fresh");
    let roaming_dir = unique_dir("legacy_fresh_roaming");
    let primary = dir.join("fem_config.json");
    let backup = dir.join("fem_config.backup.json");
    let roaming = roaming_dir.join("fem_config.json");
    std::fs::write(&primary, CONFIG).unwrap();
    let legacy = [
        legacy_temp(&primary, SystemTime::now()),
        legacy_temp(&backup, SystemTime::now()),
        legacy_temp(&roaming, SystemTime::now()),
    ];
    let stale = foreign_temp(&primary, 0, aged());

    let store = ConfigStore::new(dir.clone(), Some(roaming));
    assert!(!stale.exists(), "control: the sweep ran");
    store.save().unwrap();
    for p in &legacy {
        assert!(p.exists(), "{} was removed while fresh", p.display());
    }
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&roaming_dir);
}

/// A sweep that removed everything it tried to logs no warning at all.
#[test]
fn a_clean_sweep_logs_no_warning() {
    let dir = unique_dir("no_warn");
    let primary = dir.join("fem_config.json");
    std::fs::write(&primary, CONFIG).unwrap();
    let stale = [
        foreign_temp(&primary, 0, aged()),
        legacy_temp(&primary, aged()),
    ];

    let (store, load_warns) = warn_lines(|| ConfigStore::new(dir.clone(), None));
    assert!(stale.iter().all(|p| !p.exists()), "control: the sweep ran");
    let late = foreign_temp(&primary, 1, aged());
    let (saved, save_warns) = warn_lines(|| store.save());
    saved.unwrap();
    assert!(!late.exists(), "control: the save swept");
    assert_eq!(load_warns, Vec::<String>::new(), "load");
    assert_eq!(save_warns, Vec::<String>::new(), "save");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A directory the user cannot write makes every delete in it fail, without
/// root. The load only reads, so it must still succeed, with one warn line
/// that names the directory and nothing in it.
#[cfg(unix)]
#[test]
fn an_undeletable_temp_leaves_load_and_save_working_with_one_warn() {
    use std::os::unix::fs::PermissionsExt;
    let dir = unique_dir("undeletable");
    let roaming_dir = unique_dir("undeletable_roaming");
    let primary = dir.join("fem_config.json");
    let roaming = roaming_dir.join("fem_config.json");
    std::fs::write(&primary, r#"{"wallet_address":"W"}"#).unwrap();
    let held = foreign_temp(&primary, 0, aged());
    let held_roaming = foreign_temp(&roaming, 1, aged());
    let lock = |d: &Path, mode| std::fs::set_permissions(d, std::fs::Permissions::from_mode(mode));
    lock(&dir, 0o555).unwrap();
    lock(&roaming_dir, 0o555).unwrap();
    if std::fs::File::create(dir.join("probe")).is_ok() {
        // Permissions are not enforced (root): nothing here can fail.
        lock(&dir, 0o755).unwrap();
        lock(&roaming_dir, 0o755).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&roaming_dir);
        eprintln!("SKIPPED: directory permissions are not enforced for this user");
        return;
    }

    let (store, load_warns) = warn_lines(|| ConfigStore::new(dir.clone(), None));
    assert_eq!(store.get().wallet_address.as_deref(), Some("W"), "load");
    assert!(store.load_warning().is_none());
    assert_eq!(load_warns.len(), 1, "load: {load_warns:?}");
    let line = &load_warns[0];
    assert!(line.contains("failed=1"), "{line}");
    assert!(line.contains(&dir.display().to_string()), "{line}");
    let name = held.file_name().unwrap().to_string_lossy().to_string();
    assert!(!line.contains(&name), "the line names the file: {line}");

    // The primary's directory stays writable for the save; the roaming one
    // does not, and nothing is written there without a miner key.
    lock(&dir, 0o755).unwrap();
    let store = ConfigStore::new(dir.clone(), Some(roaming.clone()));
    let (saved, save_warns) = warn_lines(|| store.update(|c| c.wallet_address = Some("W2".into())));
    saved.unwrap();
    assert_eq!(save_warns.len(), 1, "save: {save_warns:?}");
    assert!(save_warns[0].contains(&roaming_dir.display().to_string()));
    assert!(held_roaming.exists(), "control: the delete really failed");
    assert!(
        !held.exists(),
        "control: the primary's temp went once it could"
    );

    lock(&roaming_dir, 0o755).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&roaming_dir);
}

/// Windows: another process holding a temp open without FILE_SHARE_DELETE
/// makes the delete fail. Load and save still succeed, with one warn line.
#[cfg(windows)]
#[test]
fn a_temp_held_open_leaves_load_and_save_working_with_one_warn() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = unique_dir("held");
    let primary = dir.join("fem_config.json");
    std::fs::write(&primary, CONFIG).unwrap();
    let held = foreign_temp(&primary, 0, aged());
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&held)
        .unwrap();

    let (store, load_warns) = warn_lines(|| ConfigStore::new(dir.clone(), None));
    assert_eq!(store.get().miner_key.as_deref(), Some("FEM-C7C1-TEST-KEY"));
    assert_eq!(load_warns.len(), 1, "load: {load_warns:?}");
    assert!(load_warns[0].contains("failed=1"), "{}", load_warns[0]);
    let (saved, save_warns) = warn_lines(|| store.update(|c| c.wallet_address = Some("W2".into())));
    saved.unwrap();
    assert_eq!(save_warns.len(), 1, "save: {save_warns:?}");
    assert!(held.exists(), "control: the delete really failed");

    drop(handle);
    assert_eq!(
        ConfigStore::new(dir.clone(), None)
            .get()
            .wallet_address
            .as_deref(),
        Some("W2"),
        "the save itself went through"
    );
    assert!(!held.exists(), "once released it goes");
    let _ = std::fs::remove_dir_all(&dir);
}
