//! BL-C6-3: a FEM process killed mid-save leaves its temp file
//! (`<name>.tmp.<pid>.<nanos>.<seq>`, a COMPLETE config including the miner
//! key) next to the config it was writing, and nothing ever removed it — 21
//! after 50 kills on a lab machine. After any number of interrupted saves at
//! most the temp being written may exist.
//!
//! - Loading removes every such temp an EARLIER process left, next to the
//!   primary, the backup and the roaming copy.
//! - A temp of THIS process is never removed: another store in this process
//!   may be writing it right now.
//! - Nothing that is not exactly one of those temps is touched.

use super::*;

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fem_bl3_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A pid that is not this process's.
fn other_pid(offset: u32) -> u32 {
    let own = std::process::id();
    let pid = own.wrapping_add(1 + offset);
    assert_ne!(pid, own);
    pid
}

const CONFIG: &str = r#"{"miner_key":"FEM-BL3-TEST-KEY","wallet_address":"W"}"#;

/// What a killed save leaves behind: a complete config under a temp name.
fn leave_temp(target: &Path, pid: u32, n: u32) -> PathBuf {
    let name = target.file_name().unwrap().to_string_lossy().to_string();
    let tmp = target.with_file_name(format!(
        "{name}.tmp.{pid}.{}.{n}",
        1_790_694_283_162_692_900u128 + n as u128
    ));
    std::fs::write(&tmp, CONFIG).unwrap();
    tmp
}

fn temps_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".tmp."))
        .collect();
    names.sort();
    names
}

/// The lab shape: 18 temps next to the primary and 3 next to the roaming
/// copy, from earlier processes; plus some next to the backup.
#[test]
fn temps_left_by_killed_processes_are_removed_at_load() {
    let dir = unique_dir("stale");
    let roaming_dir = unique_dir("stale_roaming");
    let primary = dir.join("fem_config.json");
    let backup = dir.join("fem_config.backup.json");
    let roaming = roaming_dir.join("fem_config.json");
    for p in [&primary, &backup, &roaming] {
        std::fs::write(p, CONFIG).unwrap();
    }
    for i in 0..18 {
        leave_temp(&primary, other_pid(i), 0);
    }
    for i in 0..4 {
        leave_temp(&backup, other_pid(i), i);
    }
    for i in 0..3 {
        leave_temp(&roaming, other_pid(20 + i), 0);
    }
    assert_eq!(temps_in(&dir).len(), 22, "control: the fixture is in place");
    assert_eq!(
        temps_in(&roaming_dir).len(),
        3,
        "control: the fixture is in place"
    );

    let store = ConfigStore::new(dir.clone(), Some(roaming.clone()));
    assert_eq!(
        temps_in(&dir),
        Vec::<String>::new(),
        "primary and backup dir after load"
    );
    assert_eq!(
        temps_in(&roaming_dir),
        Vec::<String>::new(),
        "roaming dir after load"
    );

    store
        .update(|c| c.wallet_address = Some("W2".to_string()))
        .unwrap();
    assert_eq!(temps_in(&dir), Vec::<String>::new(), "after a save");
    assert_eq!(temps_in(&roaming_dir), Vec::<String>::new(), "after a save");
    assert_eq!(store.get().miner_key.as_deref(), Some("FEM-BL3-TEST-KEY"));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&roaming_dir);
}

/// The clause itself: 50 kills, each leaving one temp, each followed by the
/// next start and a save. The temps never accumulate.
#[test]
fn after_any_number_of_interrupted_saves_at_most_one_temp_exists() {
    let dir = unique_dir("kill_loop");
    let primary = dir.join("fem_config.json");
    std::fs::write(&primary, CONFIG).unwrap();
    for kill in 0..50 {
        leave_temp(&primary, other_pid(kill), 0);
        let store = ConfigStore::new(dir.clone(), None);
        store.save().unwrap();
        let left = temps_in(&dir);
        assert!(left.len() <= 1, "after kill {kill}: {left:?}");
    }
    assert_eq!(temps_in(&dir), Vec::<String>::new());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Another store in THIS process may be mid-write: its temp stays.
#[test]
fn a_temp_of_this_process_is_never_removed() {
    let dir = unique_dir("live");
    let roaming_dir = unique_dir("live_roaming");
    let primary = dir.join("fem_config.json");
    let roaming = roaming_dir.join("fem_config.json");
    std::fs::write(&primary, CONFIG).unwrap();
    let live = leave_temp(&primary, std::process::id(), 7);
    let live_roaming = leave_temp(&roaming, std::process::id(), 8);
    let stale = leave_temp(&primary, other_pid(0), 9);

    let _store = ConfigStore::new(dir.clone(), Some(roaming.clone()));
    assert!(!stale.exists(), "control: the sweep ran");
    assert!(live.exists(), "the live temp of this process was removed");
    assert!(
        live_roaming.exists(),
        "the live roaming temp of this process was removed"
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&roaming_dir);
}

/// Only exact `<name>.tmp.<pid>.<nanos>.<seq>` siblings of a config path go.
#[test]
fn nothing_but_those_temps_is_touched() {
    let dir = unique_dir("decoys");
    let primary = dir.join("fem_config.json");
    std::fs::write(&primary, CONFIG).unwrap();
    let pid = other_pid(0);
    let stale = leave_temp(&primary, pid, 0);
    let decoys = [
        "fem_config.backup.json".to_string(),
        "fem_config.corrupt.1790694283.json".to_string(),
        "fem_config.json.tmp".to_string(),
        "fem_config.json.tmp.notes".to_string(),
        format!("fem_config.json.tmp.{pid}.12"),
        format!("fem_config.json.tmp.{pid}.12.3.4"),
        format!("fem_config.json.tmp.{pid}.x.3"),
        format!("fem_config.json.tmp.{pid}..3"),
        format!("other.json.tmp.{pid}.12.3"),
        format!("xfem_config.json.tmp.{pid}.12.3"),
    ];
    for d in &decoys {
        std::fs::write(dir.join(d), "{}").unwrap();
    }
    let _store = ConfigStore::new(dir.clone(), None);
    assert!(!stale.exists(), "control: the sweep ran");
    for d in &decoys {
        assert!(dir.join(d).exists(), "{d} was removed");
    }
    assert!(primary.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
