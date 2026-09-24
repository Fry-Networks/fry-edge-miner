//! FAIL-11 (row 6): a real RED against 5b7c8f5's product tree.
//!
//! At 5b7c8f5, `run_hardening_elevated` had exactly two callers — the boot
//! pass (main.rs) and the pre-update re-assert (updater_auto.rs) — and BOTH
//! pass `ElevationTrigger::Automatic`, which the gate refuses before any UAC
//! prompt appears. No `#[tauri::command]` anywhere in the tree passed
//! `UserClick`, so a user who wanted hardening applied (after a decline, or
//! proactively) had no way to ask for it.
//!
//! This file references NO symbol from the fix (no `retry_hardening`, no
//! `commands::hardening`) and does not `include_str!` any file the fix adds
//! — it walks the real filesystem at TEST RUNTIME via
//! `concat!(env!("CARGO_MANIFEST_DIR"), "/src")`, so it compiles and runs
//! unmodified against 5b7c8f5's product tree (where the command it looks for
//! does not exist yet) as much as against HEAD.

use std::path::{Path, PathBuf};

/// Strip line comments so prose can never satisfy an assertion — the same
/// guard used throughout this crate's other source-scan tests.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every `.rs` file under `dir`, recursively, as (path, raw content).
fn collect_rs_files(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                out.push((path, content));
            }
        }
    }
}

/// Brace-balanced extraction from the index of an opening `{`.
fn brace_balanced_body(code: &str, open_at: usize) -> &str {
    let bytes = code.as_bytes();
    let mut depth = 0i32;
    let mut i = open_at;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &code[open_at..=i];
                }
            }
            _ => {}
        }
        i += 1;
    }
    &code[open_at..]
}

/// Every `#[tauri::command]` function in `code`, as (name, body).
fn tauri_commands(code: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = code[cursor..].find("#[tauri::command]") {
        let attr_at = cursor + rel;
        cursor = attr_at + "#[tauri::command]".len();
        let Some(fn_rel) = code[cursor..].find("fn ") else {
            continue;
        };
        let fn_at = cursor + fn_rel;
        let name_start = fn_at + 3;
        let name_end = code[name_start..]
            .find(|c: char| c == '(' || c.is_whitespace())
            .map(|e| name_start + e)
            .unwrap_or(code.len());
        let name = code[name_start..name_end].trim().to_string();
        let Some(brace_rel) = code[fn_at..].find('{') else {
            continue;
        };
        let open_at = fn_at + brace_rel;
        out.push((name, brace_balanced_body(code, open_at).to_string()));
    }
    out
}

fn src_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))
}

fn all_rs_files() -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    collect_rs_files(&src_dir(), &mut files);
    files
}

/// THE fix (RED on 5b7c8f5): some `#[tauri::command]` function's body must
/// call `run_hardening_elevated(` with `ElevationTrigger::UserClick`.
#[test]
fn a_tauri_command_reaches_run_hardening_elevated_with_user_click() {
    let files = all_rs_files();
    assert!(
        !files.is_empty(),
        "control: must find .rs files under {:?}",
        src_dir()
    );
    // Control: at least one #[tauri::command] must exist at all, or the
    // scanner itself is broken and every assertion below is vacuous.
    let total_commands: usize = files
        .iter()
        .map(|(_, c)| tauri_commands(&code_only(c)).len())
        .sum();
    assert!(
        total_commands > 0,
        "control: found zero #[tauri::command] functions anywhere — the scanner is broken"
    );

    let mut user_click_commands: Vec<String> = Vec::new();
    for (_path, content) in &files {
        let code = code_only(content);
        for (name, body) in tauri_commands(&code) {
            if body.contains("run_hardening_elevated(")
                && body.contains("ElevationTrigger::UserClick")
            {
                user_click_commands.push(name);
            }
        }
    }
    assert!(
        !user_click_commands.is_empty(),
        "no #[tauri::command] anywhere under src/ calls run_hardening_elevated \
         with ElevationTrigger::UserClick — there is no user-click hardening \
         path (FAIL-11)"
    );

    // The command must be registered in main.rs's generate_handler! list, or
    // the frontend can never invoke it.
    let (_, main_rs) = files
        .iter()
        .find(|(p, _)| p.file_name().map(|n| n == "main.rs").unwrap_or(false))
        .expect("control: main.rs must be found");
    let main_code = code_only(main_rs);
    let gh_at = main_code
        .find("generate_handler![")
        .expect("control: generate_handler! must exist in main.rs");
    let gh_end = main_code[gh_at..]
        .find(']')
        .map(|e| gh_at + e)
        .expect("control: generate_handler![ must close");
    let gh_block = &main_code[gh_at..gh_end];

    let registered = user_click_commands
        .iter()
        .any(|name| gh_block.contains(&format!("::{name},")));
    assert!(
        registered,
        "the UserClick hardening command(s) {user_click_commands:?} are not \
         registered in generate_handler!, so the frontend can never invoke \
         them: {gh_block}"
    );
}

/// Do not change the boot pass's trigger, or the updater's — both must still
/// be `Automatic` after this fix. Non-discriminating on 5b7c8f5 (both already
/// say Automatic there); kept for regression protection going forward.
#[test]
fn the_boot_pass_and_the_updater_pre_update_reassert_stay_automatic() {
    let files = all_rs_files();
    for filename in ["main.rs", "updater_auto.rs"] {
        let (_, content) = files
            .iter()
            .find(|(p, _)| p.file_name().map(|n| n == filename).unwrap_or(false))
            .unwrap_or_else(|| panic!("control: {filename} must be found"));
        let code = code_only(content);
        let at = code
            .find("run_hardening_elevated(")
            .unwrap_or_else(|| panic!("{filename} must still call run_hardening_elevated"));
        let window = &code[at..(at + 500).min(code.len())];
        assert!(
            window.contains("ElevationTrigger::Automatic"),
            "{filename}'s hardening call site must stay Automatic: {window}"
        );
    }
}
