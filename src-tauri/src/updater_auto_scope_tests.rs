//! B4 defect 2: FEM killed partner processes by BARE IMAGE NAME.
//!
//! `taskkill /IM <image>` matches across the whole session and has no path
//! filter, so a user running their OWN Titan, MystNodes or Space Acres install
//! outside FEM had it force-killed — by the installer on every install or
//! update, and by `kill_startup_orphans` on every single FEM launch
//! (main.rs calls it at startup). B4's Done-when names this verbatim: "the
//! installer stops only processes whose image path is under FEM's
//! install/partner dirs".
//!
//! The image list itself is unchanged and its existing tests are untouched —
//! the images are now a secondary filter on top of the path check, so the set
//! of processes FEM stops is strictly smaller than before.

use super::{process_is_under, process_listing_script, select_orphan_pids};
use std::path::PathBuf;

/// NSIS comments start at `;`.
fn nsis_code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split(';').next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn rust_code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn fem_roots() -> Vec<PathBuf> {
    vec![
        PathBuf::from(r"C:\Users\x\AppData\Local\Fry Edge Miner"),
        PathBuf::from(r"C:\Users\x\AppData\Roaming\FryEdgeMiner\partners"),
    ]
}

#[test]
fn the_installer_hook_never_kills_by_bare_image_name() {
    // RAW source for the negative half: a comment stripper can only ever
    // REMOVE text, so checking "does not contain" over stripped text can pass
    // on a line that was merely truncated. Erring towards counting a mention
    // in a comment is the safe direction here.
    let raw = include_str!("../nsis-hooks.nsh");
    let hook = nsis_code_only(raw);
    for image in [
        "frynode.exe",
        "titan-edge.exe",
        "sdk_client.exe",
        "space-acres.exe",
    ] {
        let bare = format!("/IM {image}");
        assert!(
            !raw.contains(&bare),
            "nsis-hooks.nsh still stops {image} by bare image name — that kills \
             a user's own unrelated install of it"
        );
    }
    assert!(
        hook.contains("ExecutablePath"),
        "the installer must select the processes it stops by image PATH"
    );
    assert!(
        hook.contains("$INSTDIR"),
        "the path filter must be anchored on FEM's own install tree"
    );
}

#[test]
fn neither_orphan_sweep_still_kills_by_bare_image_name() {
    let raw = include_str!("updater_auto.rs");
    let code = rust_code_only(raw);
    // Negative assertion over RAW source — see the note above.
    assert!(
        !raw.contains("\"/IM\""),
        "the orphan sweeps must select by path, not by image name across the \
         whole session"
    );
    assert!(
        code.contains("process_is_under"),
        "the orphan sweeps must consult the path filter"
    );
}

#[test]
fn a_process_outside_fem_roots_is_never_selected() {
    let roots = fem_roots();
    assert!(
        !process_is_under(r"C:\NotFry\space-acres.exe", &roots),
        "a user's own install must never be stopped by FEM"
    );
    assert!(
        !process_is_under(r"C:\Program Files\Titan\titan-edge.exe", &roots),
        "a user's own install must never be stopped by FEM"
    );
    assert!(
        !process_is_under("", &roots),
        "a process with no reported image path is not ours to stop"
    );
    // A sibling directory that merely shares a prefix is NOT under the root.
    assert!(
        !process_is_under(
            r"C:\Users\x\AppData\Roaming\FryEdgeMiner\partners-backup\titan\titan-edge.exe",
            &roots
        ),
        "a prefix match must respect the directory boundary"
    );
}

#[test]
fn a_process_inside_fem_roots_is_selected_whatever_its_case_or_separators() {
    let roots = fem_roots();
    assert!(process_is_under(
        r"C:\Users\x\AppData\Roaming\FryEdgeMiner\partners\titan\titan-edge.exe",
        &roots
    ));
    assert!(
        process_is_under(
            r"c:\users\X\appdata\roaming\fryedgeminer\PARTNERS\titan\titan-edge.exe",
            &roots
        ),
        "Win32_Process reports whatever case the process was started with"
    );
    assert!(
        process_is_under(
            "C:/Users/x/AppData/Local/Fry Edge Miner/resources/frynode.exe",
            &roots
        ),
        "forward separators must resolve to the same directory"
    );
}

#[test]
fn only_the_in_root_pids_are_selected_from_a_listing() {
    let roots = fem_roots();
    let listing = concat!(
        "1234|C:\\Users\\x\\AppData\\Local\\Fry Edge Miner\\resources\\frynode.exe\n",
        "5678|C:\\NotFry\\frynode.exe\n",
        "9012|C:\\Users\\x\\AppData\\Roaming\\FryEdgeMiner\\partners\\titan\\titan-edge.exe\n",
        "3456|\n",
        "not-a-line\n",
    );
    assert_eq!(select_orphan_pids(listing, &roots), vec![1234, 9012]);
}

#[test]
fn the_listing_script_asks_only_for_the_images_we_own() {
    let script = process_listing_script(&["frynode.exe", "titan-edge.exe"]);
    // The per-image needles below only restate the `format!` this function was
    // just handed, so they carry little on their own. What is real logic — and
    // what a CIM filter is silently wrong without — is that several images are
    // joined with `or` rather than concatenated into one unmatchable name.
    assert!(
        script.contains("Name='frynode.exe' or Name='titan-edge.exe'"),
        "several images must be joined into a valid CIM disjunction: {script}"
    );
    assert!(
        !process_listing_script(&["frynode.exe"]).contains(" or "),
        "a single image must not emit a dangling disjunction"
    );
    assert!(script.contains("Name='frynode.exe'"));
    assert!(script.contains("Name='titan-edge.exe'"));
    assert!(
        script.contains("ExecutablePath"),
        "the listing must carry the path the filter decides on: {script}"
    );
    assert!(
        !script.contains("Stop-Process"),
        "the script only lists; the decision to stop is made in Rust so it can \
         be tested: {script}"
    );
}
