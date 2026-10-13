//! RED: the `update-failed.txt` text written by the NSIS hook must give no
//! advice to add exclusions, allow/unblock anything, bypass protections,
//! restore from quarantine, or disable any Windows security feature.
//!
//! The hook is read at RUNTIME so a missing file or marker is an assertion
//! failure, not a compile error.

use std::path::Path;

const FORBIDDEN: [&str; 16] = [
    "disable",
    "turn off",
    "exclusion",
    "allow rule",
    "allowlist",
    "exclude",
    "add an exception",
    "unblock",
    "whitelist",
    "bypass",
    "run anyway",
    "smart app control off",
    "allow ",
    "add-mppreference",
    "restore",
    "quarantine",
];

fn hook_lines() -> Vec<String> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("nsis-hooks.nsh");
    assert!(p.exists(), "src-tauri/nsis-hooks.nsh is missing");
    let s = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read hook: {e}"));
    s.lines().map(|l| l.to_string()).collect()
}

/// Lines from the `FileOpen ... update-failed.txt` line through the next `FileClose`.
fn update_failed_block(lines: &[String]) -> Vec<String> {
    let start = lines
        .iter()
        .position(|l| l.contains("FileOpen") && l.contains("update-failed.txt"));
    assert!(start.is_some(), "no FileOpen line for update-failed.txt");
    let start = start.unwrap();
    let end = lines[start..].iter().position(|l| l.contains("FileClose"));
    assert!(
        end.is_some(),
        "no FileClose after update-failed.txt FileOpen"
    );
    let block = lines[start..=start + end.unwrap()].to_vec();
    assert!(!block.is_empty(), "update-failed block is empty");
    block
}

/// Lowercased, whitespace-collapsed text of every FileWrite string literal.
fn written_text(block: &[String]) -> String {
    let writes: Vec<&String> = block.iter().filter(|l| l.contains("FileWrite")).collect();
    assert!(
        writes.len() >= 3,
        "expected >= 3 FileWrite lines, found {}",
        writes.len()
    );
    let mut parts = Vec::new();
    for l in writes {
        let a = l.find('"');
        let b = l.rfind('"');
        assert!(
            a.is_some() && b.is_some() && a != b,
            "FileWrite line has no string literal: {l}"
        );
        parts.push(l[a.unwrap() + 1..b.unwrap()].to_string());
    }
    parts
        .join(" ")
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn update_failed_text_gives_no_forbidden_advice() {
    let text = written_text(&update_failed_block(&hook_lines()));
    for word in FORBIDDEN {
        assert!(
            !text.contains(word),
            "update-failed.txt contains forbidden advice word {word:?} in: {text}"
        );
    }
}

#[test]
fn update_failed_text_keeps_heading_and_recovery_line() {
    let text = written_text(&update_failed_block(&hook_lines()));
    assert!(
        text.contains("fry edge miner"),
        "update-failed.txt lost its 'Fry Edge Miner' heading: {text}"
    );
    assert!(
        text.contains("reinstall") || text.contains("previous version") || text.contains("support"),
        "update-failed.txt has no actionable recovery line: {text}"
    );
}

#[test]
fn update_failed_target_path_unchanged() {
    let lines = hook_lines();
    let open = lines
        .iter()
        .find(|l| l.contains("FileOpen") && l.contains("update-failed.txt"));
    assert!(open.is_some(), "no FileOpen line for update-failed.txt");
    assert!(
        open.unwrap().contains("\"$INSTDIR\\update-failed.txt\""),
        "FileOpen target changed: {}",
        open.unwrap()
    );
}
