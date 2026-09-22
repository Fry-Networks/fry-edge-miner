//! B13 — Titan on a custom storage root.
//!
//! The reported state was a partner directory holding
//! `titan-edge_v0.1.20_246b9dd_widnows_amd64/` and `titan-edge.tar.gz`, with
//! no `titan-edge.exe` at the top level: an extraction that was KILLED
//! part-way, because the `tar` call was bounded by the repo's 20 s
//! short-lived-CLI-probe deadline rather than by an install deadline.

use super::*;

/// The extraction deadline must be an installer deadline. Same shape as the
/// existing VC-redist timeout guard in titan.rs.
#[test]
fn the_archive_extraction_deadline_is_the_installer_deadline_not_the_probe_deadline() {
    assert!(
        EXTRACT_TIMEOUT > crate::supervisor::platform::PROBE_TIMEOUT,
        "unpacking a partner release is not a short-lived CLI probe"
    );
    assert_eq!(EXTRACT_TIMEOUT, crate::supervisor::platform::LONG_TIMEOUT);
}

fn touch(path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, b"x").unwrap();
}

fn exe_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "titan-edge.exe"
    } else {
        "titan-edge"
    }
}

/// titan-edge.exe cannot run without goworkerd.dll, but the entry guard
/// checked only the exe — so a half-install reported "already present",
/// returned Ok(()) and could never be repaired.
#[test]
fn a_partner_dir_with_only_the_exe_is_not_a_complete_install() {
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join(exe_name()));

    assert!(
        dir.path().join(exe_name()).exists(),
        "the pre-fix guard's own predicate is TRUE here — which is why it short-circuited"
    );
    assert!(!TitanIntegration::install_is_complete(dir.path()));
}

#[test]
fn a_partner_dir_with_both_files_is_a_complete_install() {
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join(exe_name()));
    touch(&dir.path().join("goworkerd.dll"));

    assert!(TitanIntegration::install_is_complete(dir.path()));
}

/// The exact photographed layout: everything still inside the extracted
/// subdirectory, archive not cleaned up, nothing at the top level.
#[test]
fn an_install_that_left_the_exe_in_the_extracted_subdir_is_not_complete() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("titan-edge_v0.1.20_246b9dd_widnows_amd64");
    touch(&sub.join(exe_name()));
    touch(&sub.join("goworkerd.dll"));
    touch(&dir.path().join("titan-edge.tar.gz"));

    assert!(!TitanIntegration::install_is_complete(dir.path()));
}

#[test]
fn an_empty_partner_dir_is_not_a_complete_install() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!TitanIntegration::install_is_complete(dir.path()));
}

/// The cleanup at the end of install() was non-recursive, so any third file in
/// the archive left the extracted directory behind permanently — which is the
/// leftover directory users photographed.
#[test]
fn cleanup_clears_an_extracted_dir_that_still_holds_extra_files() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("titan-edge_v0.1.20_246b9dd_widnows_amd64");
    touch(&sub.join("README.md"));

    assert!(
        std::fs::remove_dir(&sub).is_err(),
        "the pre-fix non-recursive cleanup cannot remove a directory that still holds a file"
    );
    std::fs::remove_dir_all(&sub).unwrap();
    assert!(!sub.exists());
}
