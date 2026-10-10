//! RED: the bundled VC++ redist must be launched with `/passive` (not
//! `/quiet`) so the UAC consent comes to the foreground and the Burn
//! progress UI shows.
//!
//! The hook is read at RUNTIME so a missing file or marker is an assertion
//! failure, not a compile error.

use std::path::Path;

const BEGIN: &str = "; >>> FEMQA-VCREDIST";
const END: &str = "; <<< FEMQA-VCREDIST";

/// Lowercased command-line builder lines (the `StrCpy $R3 ...` lines that
/// mention the bundled redist) inside the FEMQA-VCREDIST block.
fn cmdline_lines() -> Vec<String> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("nsis-hooks.nsh");
    assert!(p.exists(), "src-tauri/nsis-hooks.nsh is missing");
    let s = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read hook: {e}"));
    let lines: Vec<&str> = s.lines().collect();
    let b = lines.iter().position(|l| l.trim() == BEGIN);
    assert!(
        b.is_some(),
        "marker line '{BEGIN}' missing from nsis-hooks.nsh"
    );
    let b = b.unwrap();
    let e = (b + 1..lines.len()).find(|&i| lines[i].trim() == END);
    assert!(
        e.is_some(),
        "marker line '{END}' missing (or before begin) in nsis-hooks.nsh"
    );
    let found: Vec<String> = lines[b + 1..e.unwrap()]
        .iter()
        .map(|l| l.trim())
        .filter(|l| {
            let l = l.to_ascii_lowercase();
            l.starts_with("strcpy $r3") && l.contains("vc_redist.x64.exe")
        })
        .map(|l| l.to_ascii_lowercase())
        .collect();
    assert!(
        !found.is_empty(),
        "no 'StrCpy $R3 ... vc_redist.x64.exe ...' command-line line found in the FEMQA-VCREDIST block"
    );
    found
}

#[test]
fn redist_cmdline_uses_passive() {
    for l in cmdline_lines() {
        assert!(
            l.contains("/passive"),
            "redist command line must contain /passive (consent to foreground + progress UI): {l}"
        );
    }
}

#[test]
fn redist_cmdline_does_not_use_quiet() {
    for l in cmdline_lines() {
        assert!(
            !l.contains("/quiet"),
            "redist command line must NOT contain /quiet (hides consent + progress UI): {l}"
        );
    }
}

#[test]
fn redist_cmdline_keeps_install_and_norestart() {
    for l in cmdline_lines() {
        assert!(
            l.contains("/install"),
            "redist command line must keep /install: {l}"
        );
        assert!(
            l.contains("/norestart"),
            "redist command line must keep /norestart: {l}"
        );
    }
}
