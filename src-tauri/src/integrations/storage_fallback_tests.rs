//! B4 defect 3: after a silent storage-root fallback, Settings reported the
//! CONFIGURED root as "where partner data is stored" while the files were
//! actually going to %APPDATA%, under a banner promising that a restart would
//! start using the configured location.
//!
//! Nothing is ever deleted on that path — `init_storage_root` only chooses a
//! different root — but a user who set D:\fry_storage, lost the drive, and then
//! opened the folder FEM still names sees an empty tree and a restart banner
//! that cannot help. That is "fry edge miner folder empty" manufactured out of
//! a `warn!` that never reached the UI.
//!
//! The resolution itself is unchanged: the `OnceLock` freeze is load-bearing
//! (a root that could flip mid-run would make `installed_version()` return None
//! and reinstall on top of a live partner). Only the reporting is added.

use super::download::storage_fallback_message;
use std::path::Path;

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_fallback_message_names_both_paths_and_says_nothing_was_deleted() {
    let msg = storage_fallback_message(
        Path::new(r"D:\fry_storage\FryEdgeMiner\partners"),
        "Fry Edge Miner could not create that folder: The system cannot find the path specified.",
        Path::new(r"C:\Users\x\AppData\Roaming\FryEdgeMiner\partners"),
    );
    assert!(
        msg.contains(r"D:\fry_storage\FryEdgeMiner\partners"),
        "the message must name the location the user configured: {msg}"
    );
    assert!(
        msg.contains("could not create that folder"),
        "the message must carry the real reason, not a generic failure: {msg}"
    );
    assert!(
        msg.contains(r"C:\Users\x\AppData\Roaming\FryEdgeMiner\partners"),
        "the message must name where the files actually are: {msg}"
    );
    assert!(
        msg.contains("have not been deleted"),
        "the one thing a user staring at an empty folder needs told: {msg}"
    );
}

#[test]
fn the_fallback_branch_records_the_reason_rather_than_only_logging_it() {
    let code = code_only(include_str!("download.rs"));
    let warn_at = code
        .find("Configured storage location is unusable")
        .expect("the fallback must still log");
    let record_at = code
        .find("STORAGE_ROOT_FALLBACK")
        .expect("the fallback must be recorded for the UI, not only logged");
    assert!(
        record_at > warn_at || code[warn_at..].contains("STORAGE_ROOT_FALLBACK"),
        "the reason must be recorded on the branch that actually falls back"
    );
}

/// The UI cannot tell the truth without both: `path` is what the user asked
/// for, `active_path` is where the files are, and they disagree exactly when
/// `fallback_reason` is set.
#[test]
fn the_settings_payload_carries_the_active_path_and_the_reason() {
    let code = code_only(include_str!("../commands/settings.rs"));
    for field in ["active_path", "fallback_reason"] {
        assert!(
            code.contains(field),
            "StorageLocation must expose {field}, or the Settings page has no \
             way to show where the files really are"
        );
    }
    assert!(
        code.contains("pending_restart: resolved != active"),
        "the existing pending_restart semantics must not change — the frontend \
         helper and its tests depend on them"
    );
}
