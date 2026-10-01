//! D-C8-1: the save path writes `<target>.tmp.<pid>` and renames it; a kill
//! between the two (or a rename AND a remove that both fail) left that temp,
//! which holds the whole config including the Iagon node token, forever.
//!
//! - Every save sweeps stale temps next to the target before it writes.
//! - Iagon's token reader sweeps once per process, before anything else.

use super::*;

/// `src` with every `//` comment cut off, so prose cannot satisfy a guard.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The function that writes the temp must sweep before it writes it.
#[test]
fn the_save_path_sweeps_stale_temps_before_writing_its_own() {
    let code = code_only(include_str!("partner_secret.rs"));
    let write = code
        .find("std::fs::write(&tmp")
        .expect("the save path still writes a temp");
    let enclosing = code[..write].rfind("fn ").expect("enclosing fn");
    assert!(
        code[enclosing..write].contains("sweep_stale_secret_temps("),
        "the save path must call sweep_stale_secret_temps( before std::fs::write(&tmp"
    );
}

/// The only load path for this secret is Iagon's `node_token()`; it must sweep
/// first, even when the token comes from the environment.
#[test]
fn iagon_node_token_sweeps_before_the_env_var_early_return() {
    let code = code_only(include_str!("../integrations/iagon.rs"));
    let start = code
        .find("fn node_token()")
        .expect("iagon.rs still has fn node_token()");
    let env = start
        + code[start..]
            .find("IAGON_NODE_TOKEN")
            .expect("node_token still reads IAGON_NODE_TOKEN");
    assert!(
        code[start..env].contains("sweep_stale_secret_temps("),
        "node_token() must call sweep_stale_secret_temps( before IAGON_NODE_TOKEN"
    );
}

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fem_c8_ps_{tag}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const CONFIG: &str = r#"{"node_token":"C8-OLD-TOKEN","extra":"kept","n":7}"#;

fn set_mtime(path: &Path, mtime: SystemTime) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(mtime).unwrap();
}

fn aged() -> SystemTime {
    SystemTime::now() - Duration::from_secs(120)
}

/// A file named `name` next to `target`, holding a config, `mtime` old.
fn plant(target: &Path, name: &str, mtime: SystemTime) -> PathBuf {
    let tmp = target.with_file_name(name);
    std::fs::write(&tmp, CONFIG).unwrap();
    set_mtime(&tmp, mtime);
    tmp
}

/// A temp another (killed) process left next to `target`.
fn foreign_temp(target: &Path, n: u32, mtime: SystemTime) -> PathBuf {
    let pid = std::process::id().wrapping_add(1 + n);
    plant(target, &format!("config.json.tmp.{pid}"), mtime)
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

fn read_config(target: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(target).unwrap()).unwrap()
}

#[test]
fn the_sweep_removes_an_aged_foreign_temp() {
    let dir = unique_dir("aged_sweep");
    let target = dir.join("config.json");
    let stale = foreign_temp(&target, 0, aged());

    let ((), warns) = warn_lines(|| sweep_stale_secret_temps(&target));
    assert!(!stale.exists(), "an aged foreign temp must go");
    assert!(warns.is_empty(), "a clean sweep logs no warn: {warns:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_save_removes_an_aged_foreign_temp_and_keeps_every_other_key() {
    let dir = unique_dir("aged_save");
    let target = dir.join("config.json");
    std::fs::write(&target, CONFIG).unwrap();
    let stale = foreign_temp(&target, 0, aged());

    let (saved, warns) = warn_lines(|| save_secret_at(&target, "node_token", "C8-NEW-TOKEN"));
    saved.unwrap();
    assert!(!stale.exists(), "the save must sweep the aged foreign temp");
    assert!(warns.is_empty(), "{warns:?}");
    let parsed = read_config(&target);
    assert_eq!(parsed["node_token"], "C8-NEW-TOKEN");
    assert_eq!(parsed["extra"], "kept");
    assert_eq!(parsed["n"], 7);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fresh_future_own_pid_and_look_alike_entries_are_kept() {
    let dir = unique_dir("kept");
    let target = dir.join("config.json");
    let kept = [
        foreign_temp(&target, 1, SystemTime::now()),
        foreign_temp(&target, 2, SystemTime::now() + Duration::from_secs(3600)),
        plant(
            &target,
            &format!("config.json.tmp.{}", std::process::id()),
            aged(),
        ),
        plant(&target, "config.json.tmp.12x", aged()),
        plant(&target, "config.json.tmp.1.2", aged()),
        plant(&target, "config.json.tmp.", aged()),
        plant(&target, "other.json.tmp.1", aged()),
    ];
    let subdir = dir.join("config.json.tmp.5");
    std::fs::create_dir(&subdir).unwrap();
    // Aged too, so only the file-type check can keep it.
    #[cfg(unix)]
    std::fs::File::open(&subdir)
        .unwrap()
        .set_modified(aged())
        .unwrap();

    let ((), warns) = warn_lines(|| sweep_stale_secret_temps(&target));
    for path in &kept {
        assert!(path.exists(), "must be kept: {}", path.display());
    }
    assert!(subdir.is_dir(), "a directory is never a temp");
    assert!(warns.is_empty(), "{warns:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_directory_is_a_silent_no_op() {
    let target = unique_dir("missing").join("absent").join("config.json");
    let ((), warns) = warn_lines(|| sweep_stale_secret_temps(&target));
    assert!(warns.is_empty(), "{warns:?}");
}

/// A delete that fails is one warn with the count and the directory, never
/// the file name, and the next save once it can clears it.
#[cfg(unix)]
#[test]
fn a_failed_delete_is_one_warn_without_the_file_name() {
    use std::os::unix::fs::PermissionsExt;
    let dir = unique_dir("locked");
    let target = dir.join("config.json");
    std::fs::write(&target, CONFIG).unwrap();
    let stale = foreign_temp(&target, 0, aged());
    let stale_name = stale.file_name().unwrap().to_string_lossy().into_owned();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();

    let ((), warns) = warn_lines(|| sweep_stale_secret_temps(&target));
    assert!(stale.exists(), "control: the delete really failed");
    assert_eq!(warns.len(), 1, "{warns:?}");
    assert!(warns[0].contains("failed=1"), "{}", warns[0]);
    assert!(
        warns[0].contains(&dir.display().to_string()),
        "{}",
        warns[0]
    );
    assert!(!warns[0].contains(&stale_name), "{}", warns[0]);

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (saved, warns) = warn_lines(|| save_secret_at(&target, "node_token", "C8-NEW-TOKEN"));
    saved.unwrap();
    assert!(!stale.exists(), "once it can, the next save clears it");
    assert!(warns.is_empty(), "{warns:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Windows: another process holding a temp open without FILE_SHARE_DELETE
/// makes the delete fail. The save still succeeds, with one warn line.
#[cfg(windows)]
#[test]
fn a_temp_held_open_leaves_the_save_working_with_one_warn() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = unique_dir("held");
    let target = dir.join("config.json");
    std::fs::write(&target, CONFIG).unwrap();
    let held = foreign_temp(&target, 0, aged());
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&held)
        .unwrap();

    let (saved, warns) = warn_lines(|| save_secret_at(&target, "node_token", "C8-NEW-TOKEN"));
    saved.unwrap();
    assert_eq!(warns.len(), 1, "{warns:?}");
    assert!(warns[0].contains("failed=1"), "{}", warns[0]);
    assert!(held.exists(), "control: the delete really failed");
    assert_eq!(read_config(&target)["node_token"], "C8-NEW-TOKEN");

    drop(handle);
    let (saved, warns) = warn_lines(|| save_secret_at(&target, "node_token", "C8-NEW-TOKEN"));
    saved.unwrap();
    assert!(warns.is_empty(), "{warns:?}");
    assert!(!held.exists(), "once released it goes");
    let _ = std::fs::remove_dir_all(&dir);
}
