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

/// G4 finding 7 — the B13 half-install repair was unreachable from every
/// production path.
///
/// Every caller of `install()` gates on `installed_version()`, which checked
/// ONLY titan-edge.exe. With the exe present and goworkerd.dll missing — the
/// reported "Bad Image … goworkerd.dll" case — it returned Some, install() was
/// skipped, start() spawned anyway, and the health loop restarted the same
/// broken tree forever with no in-app repair.
#[test]
fn installed_version_reports_a_half_install_as_not_installed() {
    let src = include_str!("titan.rs");
    let at = src
        .find(&format!("fn installed{}(&self)", "_version"))
        .expect("installed_version must exist");
    let end = src[at..]
        .find("\n    fn ")
        .map(|e| at + e)
        .unwrap_or(src.len());
    // Comments stripped: the body deliberately EXPLAINS that hashing belongs in
    // start(), and a guard that trips over its own explanation is worse than no
    // guard. (This is the second time that trap has caught me — the first was a
    // doc comment naming the OnceLock it replaced.)
    let body: String = src[at..end]
        .lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        body.contains(&format!("install_is{}(", "_complete")),
        "installed_version does not consult the completeness predicate, so the gated \
         install() callers can never repair a half-install:\n{body}"
    );
    assert!(
        !body.contains("sha256") && !body.contains("compute_sha"),
        "installed_version must not hash — it runs on the poll path:\n{body}"
    );
}

/// B15 Done-when: verified against a pinned manifest BEFORE EVERY SPAWN.
#[test]
fn start_verifies_the_pinned_files_before_spawning() {
    let src = include_str!("titan.rs");
    let at = src
        .find("    async fn start(&self) -> Result<()> {")
        .expect("start must exist");
    let end = src[at..]
        .find("\n    async fn ")
        .map(|e| at + e)
        .unwrap_or(src.len());
    let body = &src[at..end];

    let verify = format!("unverified_pinned{}(", "_files");
    let spawn = format!("start_integration{}(", "_with_env");
    let v = body
        .find(&verify)
        .expect("start must verify the pinned files");
    if let Some(s) = body.find(&spawn) {
        assert!(v < s, "the spawn happens before verification:\n{body}");
    }
}

#[test]
fn the_pins_are_full_sha256_digests_and_real_sizes() {
    for (name, digest, size) in PINNED_FILES {
        assert_eq!(digest.len(), 64, "{name}: {digest}");
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()), "{name}");
        assert!(
            digest.chars().any(|c| c != '0'),
            "{name}: an all-zero pin is a placeholder"
        );
        assert!(size > 1_000_000, "{name}: {size} is not a partner binary");
    }
}

#[test]
fn a_missing_pinned_file_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let bad = unverified_pinned_files(dir.path());
    assert_eq!(bad.len(), PINNED_FILES.len(), "{bad:?}");
    assert!(bad.iter().all(|b| b.contains("is missing")), "{bad:?}");
}

/// A truncated or substituted file is caught on size before anything is
/// hashed — which is also what keeps the common case cheap.
#[test]
fn a_file_of_the_wrong_size_is_reported_without_hashing() {
    let dir = tempfile::tempdir().unwrap();
    for (name, _, _) in PINNED_FILES {
        std::fs::write(dir.path().join(name), b"not the real partner binary").unwrap();
    }

    let bad = unverified_pinned_files(dir.path());
    assert_eq!(bad.len(), PINNED_FILES.len(), "{bad:?}");
    assert!(
        bad.iter().all(|b| b.contains("bytes, expected")),
        "a wrong-sized file must be rejected on size: {bad:?}"
    );
}

#[test]
fn an_unchanged_file_is_not_re_verified() {
    let now = std::time::SystemTime::now();
    assert!(metadata_unchanged(Some((10, now)), (10, now)));
    assert!(!metadata_unchanged(Some((10, now)), (11, now)));
    assert!(!metadata_unchanged(
        Some((10, now)),
        (10, now + std::time::Duration::from_secs(1))
    ));
    assert!(!metadata_unchanged(None, (10, now)));
}
