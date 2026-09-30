//! D-C7-1 (A7-P2): the age check itself. A candidate another writer renamed
//! or removed after the directory listing has no mtime: keep it, no error.

use super::*;
use std::time::{Duration, SystemTime};

#[test]
fn the_age_check_keeps_a_path_that_no_longer_exists() {
    let dir = std::env::temp_dir().join(format!("fem_c7c1_age_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fem_config.json.tmp.1.2.3");
    std::fs::write(&path, "{}").unwrap();
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    let now = SystemTime::now();
    file.set_modified(now - Duration::from_secs(120)).unwrap();
    assert!(ConfigStore::is_aged(&path), "control: 120 s old is aged");
    file.set_modified(now - Duration::from_secs(30)).unwrap();
    assert!(!ConfigStore::is_aged(&path), "30 s old is not");
    file.set_modified(now + Duration::from_secs(120)).unwrap();
    assert!(!ConfigStore::is_aged(&path), "a future mtime is not");
    drop(file);

    std::fs::remove_file(&path).unwrap();
    assert!(!ConfigStore::is_aged(&path), "a vanished path is kept");
    assert!(!ConfigStore::is_aged(&dir.join("never-existed")));
    let _ = std::fs::remove_dir_all(&dir);
}
