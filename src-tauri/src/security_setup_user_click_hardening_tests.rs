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

/// Brace-balanced extraction from the index of an opening `{` — STRING-
/// LITERAL-AWARE: a `{`/`}` inside a `"..."` (e.g. a `format!`/`panic!`
/// placeholder like `"...: {body}"`, or a literal `{{`/`}}`) does not count
/// toward depth, or an odd brace inside one function's assertion message
/// silently swallows every function after it into the "body" — which is
/// exactly what happened here without this: `get_hardening_status_reads_
/// the_gates_blocked_reasons`'s own short body was extracted correctly by
/// depth, but a naive (non-string-aware) counter run over the SAME source
/// from other callers overran into later functions' real
/// `run_hardening_elevated(..., UserClick)` calls and misattributed them.
fn brace_balanced_body(code: &str, open_at: usize) -> &str {
    let bytes = code.as_bytes();
    let mut depth = 0i32;
    let mut i = open_at;
    let mut in_string = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if b == b'\\' {
                i = (i + 2).min(bytes.len());
                continue;
            }
            if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => in_string = true,
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

// ---------------------------------------------------------------------
// BUG LOOP 3, item 1 (BLOCKING): `run_hardening_elevated` has no injectable
// seam — with `ElevationTrigger::UserClick` it ALWAYS spawns a real
// elevated PowerShell script (security_setup.rs's `Add-MpPreference` /
// `netsh advfirewall` script, run via `Start-Process -Verb RunAs -Wait`).
// A test that reaches it with UserClick on Windows raises a genuine UAC
// prompt and, if approved (or on an admin CI runner with UAC disabled),
// mutates the real FEM-FryNode firewall rule and adds real Defender
// exclusions. This tripwire scans EVERY test in the tree (not just this
// file) and fails if any such test is not `#[cfg(not(windows))]`.
// ---------------------------------------------------------------------

/// Is byte offset `at` inside a `"..."` string literal, counting unescaped
/// `"` from the start of `text`? Distinguishes a REAL call/identifier from a
/// source-scan test's own needle string (e.g. `.find("run_hardening_elevated(")`)
/// — exactly what several sibling tests in this crate do, and would
/// otherwise make this tripwire match its own (and its neighbors') search
/// text. Correct for this codebase's style in these files (single-line
/// string literals, no unescaped multi-line raw strings before a match).
fn is_inside_string_literal(text: &str, at: usize) -> bool {
    let mut in_string = false;
    let mut chars = text[..at].chars();
    while let Some(c) = chars.next() {
        if c == '\\' && in_string {
            chars.next();
            continue;
        }
        if c == '"' {
            in_string = !in_string;
        }
    }
    in_string
}

/// Every byte offset where `needle` occurs in `text` as CODE, not inside a
/// string literal.
fn non_string_literal_positions(text: &str, needle: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = text[cursor..].find(needle) {
        let at = cursor + rel;
        if !is_inside_string_literal(text, at) {
            out.push(at);
        }
        cursor = at + needle.len();
    }
    out
}

/// Byte offset of every `#[test]`/`#[tokio::test]` attribute in `content`.
fn test_attr_positions(content: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for needle in ["#[test]", "#[tokio::test]"] {
        let mut cursor = 0usize;
        while let Some(rel) = content[cursor..].find(needle) {
            out.push(cursor + rel);
            cursor += rel + needle.len();
        }
    }
    out.sort_unstable();
    out
}

/// Tripwire: no test anywhere under `src/` may reach `run_hardening_elevated`
/// with `UserClick` unless it is `#[cfg(not(windows))]`. Proven RED on
/// 13e6207 (see the bug-loop-3 report): the exact defect this catches was
/// live in that commit's `clearing_blocked_before_a_repeat_user_click_avoids_already_attempted`.
#[test]
fn no_unguarded_test_reaches_run_hardening_elevated_with_user_click() {
    let files = all_rs_files();
    assert!(
        !files.is_empty(),
        "control: must find .rs files under {:?}",
        src_dir()
    );

    let mut checked_dangerous = 0usize;
    let mut violations: Vec<String> = Vec::new();

    for (path, content) in &files {
        for attr_at in test_attr_positions(content) {
            // Gated? The established pattern in this crate is
            // `#[cfg(not(windows))]` on the line directly above
            // `#[test]`/`#[tokio::test]` — walk back 2 lines (always a valid
            // UTF-8 boundary, since '\n' is single-byte ASCII, unlike a
            // fixed byte-offset window which can land mid multi-byte
            // character in a doc comment's em dash).
            let mut window_start = attr_at;
            for _ in 0..2 {
                // `content[..window_start]` always ends with the newline
                // immediately before `window_start` itself (every attribute
                // starts a fresh line) — `.rfind('\n')` on it trivially
                // finds THAT SAME newline and makes no progress. Exclude it
                // first (`saturating_sub(1)`) so each iteration finds the
                // PREVIOUS line's boundary instead of re-finding the current
                // one.
                let search_end = window_start.saturating_sub(1);
                window_start = content[..search_end]
                    .rfind('\n')
                    .map(|i| i + 1)
                    .unwrap_or(0);
            }
            let is_gated = content[window_start..attr_at].contains("cfg(not(windows))");

            let Some(fn_rel) = content[attr_at..].find("fn ") else {
                continue;
            };
            let fn_at = attr_at + fn_rel;
            let Some(brace_rel) = content[fn_at..].find('{') else {
                continue;
            };
            let open = fn_at + brace_rel;
            let body = code_only(brace_balanced_body(content, open));

            // A REAL call, not a source-scan test's own needle string (e.g.
            // `.find("run_hardening_elevated(")`, which is exactly what
            // several sibling tests in this file and in
            // hardening_user_click_tests.rs do): the occurrence must be CODE,
            // not inside a `"..."` literal.
            for call_at in non_string_literal_positions(&body, "run_hardening_elevated(") {
                let window = &body[call_at..(call_at + 300).min(body.len())];
                if window.contains("UserClick") {
                    checked_dangerous += 1;
                    if !is_gated {
                        violations.push(format!(
                            "{}: a test at byte {attr_at} calls run_hardening_elevated \
                             with UserClick (a REAL elevated script on Windows) but is \
                             not #[cfg(not(windows))]",
                            path.display()
                        ));
                    }
                }
            }
        }
    }

    assert!(
        checked_dangerous >= 1,
        "control: must find at least one test that reaches run_hardening_elevated \
         with UserClick somewhere in the tree, or this tripwire is vacuous"
    );
    assert!(
        violations.is_empty(),
        "unguarded real-elevation test(s) found — every one must be \
         #[cfg(not(windows))]:\n{}",
        violations.join("\n")
    );
}
