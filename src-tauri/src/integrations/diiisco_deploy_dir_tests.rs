//! diiisco panic (row 9): `deploy_dir()` used to `.expect("no local data
//! dir")` on `dirs::data_local_dir()`'s return — reachable from `install()`,
//! `start()`, `stop()`, `installed_version()`, and `collect_poc_data()`, none
//! of which could recover from a panic mid-supervisor-tick.
//!
//! `resolve_local_deploy_dir(Option<PathBuf>)` is the pure seam the fix pulls
//! out of `deploy_dir()`: given what `dirs::data_local_dir()` returned, it
//! resolves the deploy directory or explains why it can't. A real machine
//! with $HOME set (every dev box and CI runner this crate builds on) can
//! never make `dirs::data_local_dir()` itself return `None`, so the seam is
//! what lets a `None` be driven directly, deterministically, without
//! mutating process environment (which — with tests running in parallel
//! threads of one process — could corrupt unrelated tests reading the same
//! env vars).

use super::*;

#[test]
fn a_present_local_data_dir_resolves_normally() {
    let dir = resolve_local_deploy_dir(Some(std::path::PathBuf::from(
        "/home/someuser/.local/share",
    )))
    .expect("a Some(...) input must resolve, not error");
    assert!(dir.ends_with("diiisco"), "{dir:?}");
}

/// `resolve_deploy_dir` (download.rs) prefers an EXISTING legacy path over
/// the rooted one — so a weak "ends_with diiisco" check alone can pass via
/// the rooted fallback no matter what `base` was, without ever proving the
/// injected `local_data_dir` actually reached the legacy candidate. Creating
/// a real legacy directory under a real temp `local_data_dir` makes that
/// plumbing observable: only the ACTUAL injected base can make this pick the
/// legacy path.
#[test]
fn the_injected_local_data_dir_is_what_the_legacy_candidate_is_built_from() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let legacy = tmp.path().join("FryEdgeMiner").join("diiisco");
    std::fs::create_dir_all(&legacy).expect("create legacy dir");

    let dir = resolve_local_deploy_dir(Some(tmp.path().to_path_buf()))
        .expect("a Some(...) input must resolve");
    assert_eq!(
        dir, legacy,
        "an EXISTING legacy dir built from the injected local_data_dir must win \
         over the rooted fallback — proves `base` really is threaded through, \
         not ignored"
    );
}

/// THE fix: a missing local data dir is a handled `Err`, not a panic.
#[test]
fn a_missing_local_data_dir_is_a_handled_error_not_a_panic() {
    let result = std::panic::catch_unwind(|| resolve_local_deploy_dir(None));
    match result {
        Ok(Ok(dir)) => panic!("expected an error for a missing local data dir, got Ok({dir:?})"),
        Ok(Err(e)) => {
            let msg = e.to_string().to_lowercase();
            assert!(
                msg.contains("local") && msg.contains("data"),
                "the error should explain what's missing: {e}"
            );
        }
        Err(_) => panic!(
            "resolve_local_deploy_dir(None) PANICKED instead of returning an Err — \
             this is the diiisco panic (deploy_dir's old `.expect(\"no local data dir\")`)"
        ),
    }
}

/// `deploy_dir()` itself (the zero-arg entry point every caller uses) must
/// have the same `Result` contract — not still panic somewhere between the
/// seam and its own return.
#[test]
fn deploy_dir_itself_returns_a_result() {
    // On this box $HOME is set, so this always resolves — the point is only
    // that the TYPE is `Result<PathBuf>`, not `PathBuf`, so a caller can
    // propagate a future failure instead of unwinding.
    let _: Result<std::path::PathBuf> = deploy_dir();
}

/// `compose_file()` must propagate a `deploy_dir()` failure rather than
/// panicking on it.
#[test]
fn compose_file_propagates_without_panicking() {
    // Can't drive deploy_dir() itself to None (see module doc), so this pins
    // the propagation shape: compose_file() is `Result`, and its Ok value is
    // always deploy_dir()'s Ok value plus the filename.
    let dir = deploy_dir().expect("this box has a local data dir");
    let compose = compose_file().expect("must resolve alongside deploy_dir()");
    assert_eq!(compose, dir.join("docker-compose.yml"));
}
