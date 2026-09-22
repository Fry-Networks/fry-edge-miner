//! B13 — Titan on a custom storage root.
//!
//! The reported state was a partner directory holding
//! `titan-edge_v0.1.20_246b9dd_widnows_amd64/` and `titan-edge.tar.gz`, with
//! no `titan-edge.exe` at the top level: an extraction that was KILLED
//! part-way, because the `tar` call was bounded by the repo's 20 s
//! short-lived-CLI-probe deadline rather than by an install deadline.

use super::*;

/// The constants' relationship is a build-time invariant next to
/// EXTRACT_TIMEOUT itself, because a runtime assert over two compile-time
/// constants can never fail. What is worth testing here is that the tar CALL
/// SITE actually uses it — the original version of this test compared three
/// constants and said nothing about that.
#[test]
fn the_tar_call_is_bounded_by_the_extraction_deadline() {
    let src = include_str!("titan.rs");
    let at = src
        .find(&format!("command(\"{}\")", "tar"))
        .expect("install must still extract with tar");
    let after = &src[at..];
    let bounded = after
        .find(&format!("output{}(", "_bounded"))
        .expect("the tar call must be bounded");
    let arg_end = after[bounded..]
        .find(')')
        .map(|e| bounded + e)
        .expect("the bound must have an argument");
    let arg = &after[bounded..arg_end];

    assert!(
        arg.contains("EXTRACT_TIMEOUT"),
        "the tar call is bounded by something other than the extraction deadline: {arg}"
    );
    assert!(
        !arg.contains("PROBE_TIMEOUT"),
        "the tar call is still bounded by the short-lived-CLI-probe deadline: {arg}"
    );
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

    // The pre-fix guard checked only the exe, so this state read as "already
    // present" and install() short-circuited forever.
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
/// the archive left the extracted directory behind permanently.
///
/// This drives the REAL helper install() calls. An earlier version called
/// `std::fs::remove_dir` and then `remove_dir_all` itself and asserted on the
/// state it had just produced — which tested `std::fs`, not this crate, and
/// passed on every commit.
#[tokio::test]
async fn the_cleanup_clears_a_directory_that_still_holds_extra_files() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("titan-edge_v0.1.20_246b9dd_widnows_amd64");
    touch(&sub.join(exe_name()));
    touch(&sub.join("goworkerd.dll"));
    // The file that makes a non-recursive remove fail.
    touch(&sub.join("README.md"));

    clear_extracted_dir(&sub).await;

    assert!(
        !sub.exists(),
        "the extracted directory survived the cleanup — a non-recursive remove \
         leaves exactly the leftover users reported"
    );
}

/// Clearing a directory that is already gone is the normal case on a first
/// install and must not be reported as a problem.
#[tokio::test]
async fn clearing_an_absent_directory_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("never-extracted");

    clear_extracted_dir(&absent).await;

    assert!(!absent.exists());
}
