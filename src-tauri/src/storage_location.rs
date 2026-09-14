//! BUG 1/4: validation for a user-chosen storage location.
//!
//! Everything here that can be pure is pure; exactly one function touches the
//! filesystem (`probe_writable`). Validation deliberately does NOT live in
//! serde: `ConfigStore`'s whole design is "a load must never fail" (corrupt ->
//! quarantine -> backup -> roaming -> defaults), and a field that could reject
//! a config would undermine that. A bad persisted value is reported and falls
//! back to the historic location instead.
//!
//! Deliberate non-goal: a MAPPED network drive (`Z:`) is not detected. Telling
//! it apart from a local volume needs WMI or `net use`, and a half-detection is
//! worse than none — the write probe plus the displayed free space cover it.
//! Only literal UNC syntax is rejected, which is cheap and deterministic.

use std::path::{Path, PathBuf};

/// The leaf every configured root gets, always.
///
/// This is a SAFETY boundary, not cosmetics: `aem.rs` force-clean does a
/// `remove_dir_all(partners_base_dir().join("aem"))`. If a user typed `D:\`
/// and FEM used it raw, that would target `D:\aem`.
pub const STORAGE_LEAF: [&str; 2] = ["FryEdgeMiner", "partners"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageDirError {
    NotAbsolute,
    NetworkPath,
    InsideInstallDir(String),
    NotCreatable(String),
    NotWritable(String),
}

impl StorageDirError {
    /// The exact sentence the user sees. Kept in Rust so it is testable.
    pub fn message(&self) -> String {
        match self {
            Self::NotAbsolute => {
                "Enter a full path beginning with a drive letter, for example D:\\FryEdgeMiner."
                    .to_string()
            }
            Self::NetworkPath => {
                "Network locations (\\\\server\\share) are not supported. Map the share to a \
                 drive letter or choose a local disk."
                    .to_string()
            }
            Self::InsideInstallDir(dir) => format!(
                "That folder is inside the Fry Edge Miner program folder and would be erased by \
                 the next app update. Choose a folder outside {dir}."
            ),
            Self::NotCreatable(e) => format!("Fry Edge Miner could not create that folder: {e}."),
            Self::NotWritable(e) => format!(
                "Fry Edge Miner cannot write to that folder: {e}. Choose a folder you own, or \
                 run Fry Edge Miner as an administrator."
            ),
        }
    }
}

/// A literal UNC path. Rejected because `probe_disk_gb` returns `None` for UNC
/// and `None` makes `evaluate_requirements` SKIP the disk check — so Iagon's
/// 900 GB gate would silently pass on a share FEM cannot even measure.
pub fn is_unc(path: &str) -> bool {
    let p = path.trim();
    p.starts_with("\\\\") || p.starts_with("//")
}

/// Requires the `X:\` shape — i.e. exactly what `probe_disk_gb` can read.
pub fn has_drive_letter(path: &str) -> bool {
    crate::system_info::drive_letter(Path::new(path.trim())).is_some()
}

/// Normalized, case-insensitive, component-boundary-aware containment.
///
/// The boundary check matters: `...\Fry Edge Miner Data` must NOT be treated as
/// living inside `...\Fry Edge Miner`.
pub fn is_inside_install_dir(candidate: &str, install_dir: &Path) -> bool {
    fn norm(s: &str) -> String {
        s.trim()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    }
    let c = norm(candidate);
    let i = norm(&install_dir.to_string_lossy());
    if i.is_empty() {
        return false;
    }
    c == i || c.starts_with(&format!("{i}\\"))
}

/// `<user path>/FryEdgeMiner/partners`, idempotent if the leaf is already
/// there, so re-saving a path FEM displayed does not double it.
pub fn storage_root_for(user_path: &str) -> PathBuf {
    let trimmed = user_path.trim().trim_end_matches(['\\', '/']);
    // `"D:\"` trimmed to `"D:"` is a DRIVE-RELATIVE path on Windows, so
    // pushing onto it yields `D:FryEdgeMiner` (relative to the current
    // directory on D:) rather than `D:\FryEdgeMiner`. Put the root back.
    let normalized = if trimmed.ends_with(':') {
        format!("{trimmed}\\")
    } else {
        trimmed.to_string()
    };
    let mut p = PathBuf::from(normalized);
    let already = p
        .components()
        .rev()
        .take(2)
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect::<Vec<_>>();
    if already.len() == 2
        && already[0] == STORAGE_LEAF[1].to_lowercase()
        && already[1] == STORAGE_LEAF[0].to_lowercase()
    {
        return p;
    }
    for leaf in STORAGE_LEAF {
        p.push(leaf);
    }
    p
}

/// All the pure rules, in the order the user should hear about them.
pub fn validate_pure(user_path: &str, install_dir: &Path) -> Result<(), StorageDirError> {
    let p = user_path.trim();
    if is_unc(p) {
        return Err(StorageDirError::NetworkPath);
    }
    if !has_drive_letter(p) {
        return Err(StorageDirError::NotAbsolute);
    }
    if is_inside_install_dir(p, install_dir) {
        return Err(StorageDirError::InsideInstallDir(
            install_dir.to_string_lossy().to_string(),
        ));
    }
    Ok(())
}

/// The ONLY filesystem-touching function here.
///
/// An actual write is the only reliable answer on Windows:
/// `metadata().permissions().readonly()` reports the FILE_ATTRIBUTE_READONLY
/// flag, not the ACL, and says nothing about whether THIS user can write. A
/// real write also catches a read-only mounted volume, a BitLocker-locked
/// drive, and a Defender Controlled-Folder-Access block.
pub fn probe_writable(root: &Path) -> Result<(), StorageDirError> {
    std::fs::create_dir_all(root).map_err(|e| StorageDirError::NotCreatable(e.to_string()))?;
    // `tempfile` is already a normal dependency; its Drop guard means a panic
    // between create and delete cannot leave litter behind.
    let f = tempfile::NamedTempFile::new_in(root)
        .map_err(|e| StorageDirError::NotWritable(e.to_string()))?;
    f.close()
        .map_err(|e| StorageDirError::NotWritable(e.to_string()))
}

#[cfg(test)]
mod validation_tests {
    use super::*;

    #[test]
    fn a_unc_path_is_rejected_before_it_can_fail_open() {
        assert!(is_unc(r"\\nas\share"));
        assert!(is_unc("//nas/share"));
        assert_eq!(
            validate_pure(r"\\nas\share\fem", Path::new(r"C:\Program Files\FEM")),
            Err(StorageDirError::NetworkPath)
        );
        // WHY it must be caught here: the probe cannot read it, and an
        // unmeasurable disk makes the 900 GB gate pass rather than fail.
        assert_eq!(
            crate::system_info::drive_letter(Path::new(r"\\nas\share")),
            None
        );
    }

    #[test]
    fn a_relative_or_drive_less_path_is_rejected() {
        for p in ["partners", r"..\partners", "/var/lib/fem", ""] {
            assert_eq!(
                validate_pure(p, Path::new(r"C:\Program Files\FEM")),
                Err(StorageDirError::NotAbsolute),
                "{p:?} must be rejected"
            );
        }
    }

    #[test]
    fn a_path_inside_the_install_dir_is_rejected() {
        let install = Path::new(r"C:\Users\u\AppData\Local\Fry Edge Miner");
        assert!(is_inside_install_dir(
            r"C:\Users\u\AppData\Local\Fry Edge Miner\data",
            install
        ));
        // case-insensitive
        assert!(is_inside_install_dir(
            r"c:\users\u\appdata\local\fry edge miner\x",
            install
        ));
        // and the exact directory itself
        assert!(is_inside_install_dir(
            r"C:\Users\u\AppData\Local\Fry Edge Miner",
            install
        ));
    }

    #[test]
    fn a_sibling_directory_with_a_shared_prefix_is_not_inside_the_install_dir() {
        let install = Path::new(r"C:\Users\u\AppData\Local\Fry Edge Miner");
        assert!(
            !is_inside_install_dir(r"C:\Users\u\AppData\Local\Fry Edge Miner Data", install),
            "component-boundary bug: a naive starts_with would wrongly match this"
        );
    }

    #[test]
    fn a_normal_local_path_passes_every_pure_rule() {
        assert_eq!(
            validate_pure(
                r"D:\FryEdgeMiner",
                Path::new(r"C:\Users\u\AppData\Local\Fry Edge Miner")
            ),
            Ok(())
        );
    }

    #[test]
    fn a_configured_root_always_ends_in_the_fem_leaf() {
        assert_eq!(
            storage_root_for(r"D:\"),
            PathBuf::from(r"D:\FryEdgeMiner\partners")
        );
        assert_eq!(
            storage_root_for(r"D:\Stuff"),
            PathBuf::from(r"D:\Stuff\FryEdgeMiner\partners")
        );
    }

    #[test]
    fn re_saving_a_path_fem_displayed_does_not_double_the_leaf() {
        let once = storage_root_for(r"D:\Stuff");
        let twice = storage_root_for(&once.to_string_lossy());
        assert_eq!(once, twice, "storage_root_for must be idempotent");
    }

    #[test]
    fn each_rejection_tells_the_user_what_to_do_next() {
        let cases = [
            StorageDirError::NotAbsolute,
            StorageDirError::NetworkPath,
            StorageDirError::InsideInstallDir(r"C:\X".to_string()),
            StorageDirError::NotCreatable("os error 5".to_string()),
            StorageDirError::NotWritable("os error 5".to_string()),
        ];
        for c in cases {
            let m = c.message();
            assert!(m.len() > 30, "message too terse to act on: {m}");
            let lower = m.to_lowercase();
            assert!(
                ["choose", "enter", "map the share", "could not create"]
                    .iter()
                    .any(|verb| lower.contains(verb)),
                "message gives the user no next step: {m}"
            );
        }
    }
}

#[cfg(test)]
mod write_probe_tests {
    use super::*;

    #[test]
    fn a_writable_directory_passes_and_leaves_nothing_behind() {
        let dir = std::env::temp_dir().join(format!("fem-wp-{}", std::process::id()));
        assert_eq!(probe_writable(&dir), Ok(()));
        let left: Vec<_> = std::fs::read_dir(&dir).expect("dir").collect();
        assert!(left.is_empty(), "probe left litter behind: {left:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_underneath_a_file_cannot_be_created() {
        let file = std::env::temp_dir().join(format!("fem-wp-file-{}", std::process::id()));
        std::fs::write(&file, b"x").expect("fixture file");
        let under = file.join("sub");
        assert!(
            matches!(
                probe_writable(&under),
                Err(StorageDirError::NotCreatable(_))
            ),
            "creating a directory under a FILE must fail"
        );
        let _ = std::fs::remove_file(&file);
    }
}
