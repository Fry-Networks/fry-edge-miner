use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use anyhow::Result;

use crate::config::FemConfig;

/// Thread-safe configuration store backed by a JSON file.
///
/// B7 hardening: corrupt files are quarantined (never panic), every load
/// failure falls through Local backup → Roaming secondary → defaults with a
/// visible warning, and every save is atomic (tmp + rename) with redundant
/// copies so a wiped Local profile (Hyper-V enhanced-session, roaming resets)
/// no longer loses the miner key/wallet binding.
pub struct ConfigStore {
    config: RwLock<FemConfig>,
    path: PathBuf,
    backup_path: PathBuf,
    roaming_path: Option<PathBuf>,
    load_warning: RwLock<Option<String>>,
}

impl ConfigStore {
    /// `roaming_path`: an explicit, caller-supplied mirror path (read
    /// fallback + write-through), or `None` for a fully isolated store that
    /// can never touch anything outside `config_dir`. The one production
    /// call site (`main.rs`) passes
    /// `dirs::config_dir().map(|d| d.join("FryEdgeMiner").join("fem_config.json"))`
    /// — byte-identical to this constructor's old ambient behavior.
    ///
    /// Previously this was resolved internally via `dirs::config_dir()` on
    /// *every* call, regardless of what `config_dir` was. Constructing a
    /// store against an unrelated directory (a test's temp dir, which has
    /// nothing to load) silently fell through to that ambient path and then
    /// wrote back over the REAL device's
    /// `%APPDATA%\FryEdgeMiner\fem_config.json` — corrupting a live device's
    /// miner_key in one documented incident. The roaming mirror is now
    /// structurally unreachable unless the caller opts in by passing
    /// `Some(path)`.
    pub fn new(config_dir: PathBuf, roaming_path: Option<PathBuf>) -> Self {
        let path = config_dir.join("fem_config.json");
        let backup_path = config_dir.join("fem_config.backup.json");

        tracing::info!(
            config_dir = %config_dir.display(),
            config_exists = path.exists(),
            "ConfigStore: resolving config file"
        );

        let mut warning: Option<String> = None;
        let mut recovered_from: Option<&'static str> = None;

        let mut config = Self::try_load(&path, &mut warning, "primary");
        if config.is_none() {
            config = Self::try_load(&backup_path, &mut warning, "backup");
            if config.is_some() {
                recovered_from = Some("backup");
            }
        }
        if config.is_none() {
            if let Some(ref rp) = roaming_path {
                config = Self::try_load(rp, &mut warning, "roaming");
                if config.is_some() {
                    recovered_from = Some("roaming copy");
                }
            }
        }

        let had_any_file = path.exists()
            || backup_path.exists()
            || roaming_path.as_ref().map(|p| p.exists()).unwrap_or(false)
            || warning.is_some();

        let config = match config {
            Some(cfg) => {
                if let Some(src) = recovered_from {
                    warning = Some(format!(
                        "Settings were recovered from the {src} after the main config file was missing or unreadable. Verify your miner key and wallet in Settings."
                    ));
                    tracing::warn!(source = src, "ConfigStore: recovered config");
                }
                cfg
            }
            None => {
                if had_any_file {
                    warning = Some(
                        "Your saved settings could not be read and were reset. If this device was registered before, re-enter your existing miner key and wallet — do NOT register a new key. Corrupt files were kept next to the config for support."
                            .to_string(),
                    );
                    tracing::error!(
                        "ConfigStore: all config copies unreadable — starting with defaults"
                    );
                } else {
                    tracing::info!(
                        "ConfigStore: no config file found, using defaults (new install)"
                    );
                }
                FemConfig::default()
            }
        };

        tracing::info!(
            has_miner_key = config.miner_key.is_some(),
            has_install_id = config.install_id.is_some(),
            has_wallet = config.wallet_address.is_some(),
            "ConfigStore: loaded config"
        );

        let store = Self {
            config: RwLock::new(config),
            path,
            backup_path,
            roaming_path,
            load_warning: RwLock::new(warning),
        };
        // F1: only re-persist at boot when we ACTUALLY recovered from a backup or
        // roaming copy (so the missing primary gets rebuilt). Rewriting an
        // already-good primary config on every single launch churned the file
        // and clobbered any hand-edits the user had made — the "fem_config.json
        // overwritten on every restart" report. A clean primary load leaves the
        // file untouched.
        let recovered = recovered_from.is_some();
        if recovered && store.get().miner_key.is_some() {
            if let Err(e) = store.save() {
                tracing::warn!(error = %e, "ConfigStore: initial re-persist failed");
            }
        }
        store
    }

    /// Attempt to load one config file. Corrupt files are quarantined with a
    /// timestamped `.corrupt.` name so they can be inspected — never deleted.
    fn try_load(path: &PathBuf, warning: &mut Option<String>, label: &str) -> Option<FemConfig> {
        if !path.exists() {
            return None;
        }
        match std::fs::read_to_string(path) {
            Ok(data) => match serde_json::from_str::<FemConfig>(&data) {
                Ok(cfg) => Some(cfg),
                Err(e) => {
                    tracing::error!(file = %path.display(), error = %e, "ConfigStore: corrupt config ({label})");
                    let quarantine = path.with_file_name(format!(
                        "fem_config.corrupt.{}.json",
                        chrono::Utc::now().timestamp()
                    ));
                    if let Err(qe) = std::fs::rename(path, &quarantine) {
                        tracing::warn!(error = %qe, "ConfigStore: quarantine failed");
                    }
                    if warning.is_none() {
                        *warning = Some(format!(
                            "The {label} settings file was corrupt and has been quarantined ({}).",
                            quarantine.display()
                        ));
                    }
                    None
                }
            },
            Err(e) => {
                tracing::warn!(file = %path.display(), error = %e, "ConfigStore: read failed ({label})");
                None
            }
        }
    }

    /// Warning produced during load (corrupt/recovered/reset), for the UI.
    pub fn load_warning(&self) -> Option<String> {
        self.load_warning.read().unwrap().clone()
    }

    /// Get a clone of the current configuration
    pub fn get(&self) -> FemConfig {
        self.config.read().unwrap().clone()
    }

    /// Update the configuration using a closure and persist to disk
    pub fn update<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce(&mut FemConfig),
    {
        let mut config = self.config.write().unwrap();
        f(&mut config);
        self.save_to_disk(&config)
    }

    /// Persist current config to disk
    pub fn save(&self) -> Result<()> {
        let config = self.config.read().unwrap();
        self.save_to_disk(&config)
    }

    fn save_to_disk(&self, config: &FemConfig) -> Result<()> {
        let data = serde_json::to_string_pretty(config)?;
        Self::write_atomic(&self.path, &data)?;
        // Redundant copies only once a real registration exists.
        if config.miner_key.is_some() {
            if let Err(e) = Self::write_atomic(&self.backup_path, &data) {
                tracing::warn!(error = %e, "ConfigStore: backup write failed");
            }
            if let Some(ref rp) = self.roaming_path {
                if let Err(e) = Self::write_atomic(rp, &data) {
                    tracing::warn!(error = %e, "ConfigStore: roaming write failed");
                }
            }
        }
        Ok(())
    }

    /// The temp file one `write_atomic` call writes through. Pure, so the
    /// uniqueness invariant is directly testable.
    ///
    /// B2: the old name was `path.with_extension("json.tmp")` — a pure function
    /// of the TARGET, so `fem_config.json` always became
    /// `fem_config.json.tmp`. With no single-instance guard two FEM processes
    /// wrote that same temp file, and B's `CREATE_ALWAYS` truncation landing
    /// between A's write and A's rename publishes a truncated config. The pid
    /// makes the name unique across processes, and the counter makes it unique
    /// across calls even where the clock's resolution would not.
    fn tmp_path_for(path: &Path) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "fem_config.json".to_string());
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        path.with_file_name(format!(
            "{name}.tmp.{}.{nanos}.{seq}",
            std::process::id()
        ))
    }

    /// Write via tmp-file + flush + rename so a crash mid-write can never
    /// leave a truncated config behind, and a power cut can never leave a
    /// durable rename pointing at bytes that never reached the disk.
    fn write_atomic(path: &PathBuf, data: &str) -> Result<()> {
        use std::io::Write;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = Self::tmp_path_for(path);
        let mut file = std::fs::File::create(&tmp)?;
        let mut outcome = file.write_all(data.as_bytes());
        if outcome.is_ok() {
            outcome = file.sync_all();
        }
        drop(file);
        if let Err(e) = outcome {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        if let Err(e) = std::fs::rename(&tmp, path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        if let Some(parent) = path.parent() {
            if let Ok(dir) = std::fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod isolation_tests {
    use super::*;

    /// Bug 1 regression: a store constructed against an isolated directory
    /// with no roaming mirror (`new(dir, None)`) must be structurally
    /// incapable of reading OR writing any file outside that directory, even
    /// when a real `%APPDATA%/FryEdgeMiner/fem_config.json` exists on the
    /// machine running the test. Previously `ConfigStore::new` took only
    /// `config_dir` and always resolved `dirs::config_dir()` itself — so a
    /// temp-dir store with nothing to load would fall through to, and then
    /// overwrite, the real device's config. This test proves that ambient
    /// path is now unreachable.
    ///
    /// Deliberately hermetic: no `std::env::set_var`/`remove_var` anywhere in
    /// this test. `dirs::config_dir()` is read once, read-only, to locate
    /// whatever the REAL roaming path already is on this machine (so the
    /// assertion has something concrete to compare against) — never mutated.
    /// This keeps the test race-free under `cargo test`'s default parallel
    /// threads and 100x-clean without `--test-threads=1`, unlike a
    /// `set_var`-based approach, which would reintroduce the same
    /// process-global-env hazard class as the pre-existing
    /// `FRYNODE_REGION` flake documented in this run's baseline.
    #[test]
    fn isolated_store_never_touches_a_file_outside_its_own_dir() {
        let unique = format!(
            "fem_store_isolation_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let dir = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&dir).unwrap();

        // A real roaming path exists on this machine (or would, if this ran
        // on the live test box) — snapshot it before touching anything, so
        // we can prove it is byte-identical (or still absent) afterward.
        // Read-only use of dirs::config_dir(); never mutated.
        let real_roaming =
            dirs::config_dir().map(|d| d.join("FryEdgeMiner").join("fem_config.json"));
        let before = real_roaming
            .as_ref()
            .filter(|p| p.exists())
            .map(|p| std::fs::read(p).unwrap());

        let store = ConfigStore::new(dir.clone(), None);
        store
            .update(|cfg| {
                cfg.miner_key = Some("FEM-ISOLATION-TEST-MUST-NEVER-ESCAPE".to_string());
                cfg.wallet_address = Some("test-wallet".to_string());
            })
            .unwrap();
        // A second save (the "redundant copy" path) must also stay contained.
        store.save().unwrap();

        let after = real_roaming
            .as_ref()
            .filter(|p| p.exists())
            .map(|p| std::fs::read(p).unwrap());
        assert_eq!(
            before, after,
            "an isolated ConfigStore (roaming_path=None) must never create, modify, or delete the real roaming config"
        );

        // The isolated dir itself must hold exactly what we wrote — confirms
        // this isn't merely "didn't write anywhere," it wrote to the right
        // (and only the right) place.
        let isolated_primary = dir.join("fem_config.json");
        assert!(
            isolated_primary.exists(),
            "primary save must land inside the isolated dir"
        );
        let saved: FemConfig =
            serde_json::from_str(&std::fs::read_to_string(&isolated_primary).unwrap()).unwrap();
        assert_eq!(
            saved.miner_key.as_deref(),
            Some("FEM-ISOLATION-TEST-MUST-NEVER-ESCAPE")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The explicit-mirror half of the same fix: when a caller DOES pass a
    /// roaming path, writes land there (and only there) — proving `None`
    /// above is "opted out," not "the write silently succeeded elsewhere."
    #[test]
    fn explicit_roaming_path_is_honoured_when_the_caller_opts_in() {
        let unique = format!(
            "fem_store_roaming_opt_in_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let base = std::env::temp_dir().join(unique);
        let primary_dir = base.join("primary");
        let roaming_dir = base.join("roaming");
        std::fs::create_dir_all(&primary_dir).unwrap();
        std::fs::create_dir_all(&roaming_dir).unwrap();
        let roaming_file = roaming_dir.join("fem_config.json");

        let store = ConfigStore::new(primary_dir.clone(), Some(roaming_file.clone()));
        store
            .update(|cfg| cfg.miner_key = Some("FEM-ROAMING-OPT-IN".to_string()))
            .unwrap();

        assert!(
            roaming_file.exists(),
            "explicit roaming path must receive the redundant copy"
        );
        let saved: FemConfig =
            serde_json::from_str(&std::fs::read_to_string(&roaming_file).unwrap()).unwrap();
        assert_eq!(saved.miner_key.as_deref(), Some("FEM-ROAMING-OPT-IN"));

        let _ = std::fs::remove_dir_all(&base);
    }
}

/// B2 defect 2: `write_atomic` was neither cross-process safe nor crash
/// durable, and the Done-when spells out what it has to be — "config writes
/// atomic (temp + flush + rename)".
///
/// The old implementation derived the temp name from the target
/// (`fem_config.json` -> `fem_config.json.tmp`), so two FEM processes wrote the
/// same file, and it used `std::fs::write`, which opens/writes/closes with no
/// `sync_all` — the "flush" step of the Done-when did not exist at all. This
/// function is byte-identical at v0.4.12, v0.4.25, v0.4.29, v0.4.31 and
/// v0.4.33, so it is a standing defect rather than a 0.4.29->0.4.33 regression.
#[cfg(test)]
mod atomic_write_tests {
    use super::*;

    fn unique_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fem_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The deterministic RED. `path.with_extension("json.tmp")` is a pure
    /// function of the target, so two calls for the same target returned the
    /// same path — which is exactly what let two processes collide.
    #[test]
    fn the_temp_file_name_is_unique_per_call() {
        let target = PathBuf::from("/tmp/fem-unique-test/fem_config.json");
        let first = ConfigStore::tmp_path_for(&target);
        let second = ConfigStore::tmp_path_for(&target);
        assert_ne!(
            first, second,
            "two writes to the same config must not share one temp file"
        );
        let name = first.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            name.contains(&std::process::id().to_string()),
            "the temp name must carry the pid so a second FEM process cannot \
             collide with this one: {name}"
        );
        assert!(
            name.starts_with("fem_config.json.tmp."),
            "the temp file must stay recognisably a temp of its target: {name}"
        );
        assert_eq!(
            first.parent(),
            target.parent(),
            "the temp must be written next to its target so the rename stays \
             within one filesystem and therefore stays atomic"
        );
    }

    /// The other deterministic RED, asserted against the real source: the
    /// bytes have to be on disk BEFORE the rename publishes them, or a power
    /// cut leaves a durable rename pointing at a hole.
    #[test]
    fn the_atomic_write_flushes_before_it_renames() {
        let src = include_str!("store.rs");
        let code: String = src
            .lines()
            .map(|l| l.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        let at = code
            .find("fn write_atomic")
            .expect("write_atomic must still exist");
        let body_end = code[at..]
            .find("#[cfg(test)]")
            .map(|e| at + e)
            .unwrap_or(code.len());
        let body = &code[at..body_end];
        let flush = body.find("sync_all").expect(
            "write_atomic must flush the file to disk before it renames — \
             std::fs::write does not, and the Done-when requires temp + flush + rename",
        );
        let rename = body
            .find("rename")
            .expect("write_atomic must still publish by rename");
        assert!(
            flush < rename,
            "the flush must happen BEFORE the rename, not after"
        );
    }

    #[test]
    fn no_temp_files_survive_a_successful_save() {
        let dir = unique_dir("atomic_no_orphans");
        let store = ConfigStore::new(dir.clone(), None);
        for i in 0..25 {
            store
                .update(|cfg| cfg.miner_key = Some(format!("FEM-ORPHAN-TEST-{i}")))
                .unwrap();
        }
        let leftovers: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "a successful save must leave no temp file behind: {leftovers:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Corroborating, not the RED/GREEN gate: the two tests above are the
    /// deterministic ones. This is the shape of the real-world failure — two
    /// FEM processes writing one config directory — and it must never publish
    /// a `fem_config.json` that does not parse.
    #[test]
    fn two_concurrent_writers_never_publish_an_unparseable_config() {
        use std::sync::Arc;

        let dir = unique_dir("atomic_two_writers");
        let primary = dir.join("fem_config.json");
        let a = Arc::new(ConfigStore::new(dir.clone(), None));
        let b = Arc::new(ConfigStore::new(dir.clone(), None));

        let writer = |store: Arc<ConfigStore>, tag: &'static str, pad: usize| {
            std::thread::spawn(move || {
                for i in 0..150 {
                    store
                        .update(|cfg| {
                            cfg.miner_key = Some(format!("FEM-{tag}-{i}"));
                            cfg.wallet_address = Some("W".repeat(pad));
                        })
                        .unwrap();
                }
            })
        };
        let ha = writer(Arc::clone(&a), "A", 8);
        let hb = writer(Arc::clone(&b), "B", 512);

        // Sampled rather than spun: a hot read loop would contend with the
        // writers' renames on Windows and turn a real invariant into a flake.
        let mut checked = 0usize;
        while !ha.is_finished() || !hb.is_finished() {
            if let Ok(text) = std::fs::read_to_string(&primary) {
                serde_json::from_str::<FemConfig>(&text).unwrap_or_else(|e| {
                    panic!("published a config that does not parse ({e}): {text}")
                });
                checked += 1;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        ha.join().unwrap();
        hb.join().unwrap();

        let text = std::fs::read_to_string(&primary).unwrap();
        serde_json::from_str::<FemConfig>(&text).expect("final config must parse");
        assert!(
            checked > 0,
            "the reader never observed the config while both writers were running"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
