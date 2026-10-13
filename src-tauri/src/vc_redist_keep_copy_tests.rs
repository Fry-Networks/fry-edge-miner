//! RED (C2a): the bundled VC++ redist copy is deleted ONLY when a fresh
//! post-hook probe verifies the runtime (Installed=1 in the 64-bit view,
//! Bld >= 35211, all 4 DLLs in the real System32). Every other outcome keeps it.
//!
//! The hook is read at RUNTIME so a missing file or marker is an assertion
//! failure, not a compile error.

use std::path::Path;

const BEGIN: &str = "; >>> FEMQA-VCREDIST";
const END: &str = "; <<< FEMQA-VCREDIST";
const PP_BEGIN: &str = "; >>> FEMQA-POSTPROBE";
const PP_END: &str = "; <<< FEMQA-POSTPROBE";
const DLLS: [&str; 4] = [
    "vcruntime140.dll",
    "vcruntime140_1.dll",
    "msvcp140.dll",
    "msvcp140_atomic_wait.dll",
];

fn hook_lines() -> Vec<String> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("nsis-hooks.nsh");
    assert!(p.exists(), "src-tauri/nsis-hooks.nsh is missing");
    let s = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read hook: {e}"));
    s.lines().map(|l| l.to_string()).collect()
}

/// Index of the first line whose trimmed text equals `marker` exactly
/// (so "; <<< FEMQA-VCREDIST" never matches the "-POSTPROBE" markers).
fn marker_idx(lines: &[String], marker: &str, from: usize) -> Option<usize> {
    (from..lines.len()).find(|&i| lines[i].trim() == marker)
}

/// (begin, end) line indices of the FEMQA-VCREDIST block (marker lines).
fn block_range(lines: &[String]) -> (usize, usize) {
    let b = marker_idx(lines, BEGIN, 0);
    assert!(
        b.is_some(),
        "marker line '{BEGIN}' missing from nsis-hooks.nsh"
    );
    let b = b.unwrap();
    let e = marker_idx(lines, END, b + 1);
    assert!(
        e.is_some(),
        "marker line '{END}' missing (or before begin) in nsis-hooks.nsh"
    );
    (b, e.unwrap())
}

/// (begin, end) line indices of the POSTPROBE region, both inside the block.
fn probe_range(lines: &[String]) -> (usize, usize) {
    assert!(
        !PP_BEGIN.contains("FEMQA-VCREDIST") && !PP_END.contains("FEMQA-VCREDIST"),
        "POSTPROBE marker names must not contain 'FEMQA-VCREDIST' (would confuse the bundle tests' block_of)"
    );
    let (b, e) = block_range(lines);
    let pb = marker_idx(lines, PP_BEGIN, b + 1);
    assert!(
        pb.is_some() && pb.unwrap() < e,
        "marker line '{PP_BEGIN}' missing inside the FEMQA-VCREDIST block"
    );
    let pb = pb.unwrap();
    assert!(
        !lines[pb].contains("FEMQA-VCREDIST"),
        "POSTPROBE begin marker line must not contain 'FEMQA-VCREDIST': '{}'",
        lines[pb].trim()
    );
    let pe = marker_idx(lines, PP_END, pb + 1);
    assert!(
        pe.is_some() && pe.unwrap() < e,
        "marker line '{PP_END}' missing after '{PP_BEGIN}' inside the FEMQA-VCREDIST block"
    );
    let pe = pe.unwrap();
    assert!(
        !lines[pe].contains("FEMQA-VCREDIST"),
        "POSTPROBE end marker line must not contain 'FEMQA-VCREDIST': '{}'",
        lines[pe].trim()
    );
    (pb, pe)
}

fn is_redist_delete(l: &str) -> bool {
    let l = l.to_lowercase();
    l.contains("delete") && l.contains("vc_redist.x64.exe")
}

#[test]
fn postprobe_region_follows_the_wait() {
    let lines = hook_lines();
    let (b, e) = block_range(&lines);
    let (pb, _pe) = probe_range(&lines);
    let last_wait = (b..e)
        .rev()
        .find(|&i| lines[i].contains("WaitForSingleObject"));
    assert!(
        last_wait.is_some(),
        "no line containing WaitForSingleObject in the FEMQA-VCREDIST block"
    );
    assert!(
        pb > last_wait.unwrap(),
        "POSTPROBE region (line {}) must start AFTER the last WaitForSingleObject line ({})",
        pb + 1,
        last_wait.unwrap() + 1
    );
}

#[test]
fn postprobe_region_probes_runtime_fresh() {
    let lines = hook_lines();
    let (pb, pe) = probe_range(&lines);
    let region = lines[pb..=pe].join("\n");
    let low = region.to_lowercase();
    for needle in [
        "SetRegView 64",
        "SetRegView lastused",
        "SOFTWARE\\Microsoft\\VisualStudio\\14.0\\VC\\Runtimes\\x64",
        "Installed",
        "Bld",
        "35211",
        "${DisableX64FSRedirection}",
        "${EnableX64FSRedirection}",
    ] {
        assert!(
            region.contains(needle),
            "POSTPROBE region lacks required text '{needle}'"
        );
    }
    for dll in DLLS {
        assert!(
            low.contains(dll),
            "POSTPROBE region lacks DLL check for '{dll}' (case-insensitive)"
        );
    }
}

#[test]
fn single_delete_inside_postprobe_after_checks() {
    let lines = hook_lines();
    let (b, e) = block_range(&lines);
    let (pb, pe) = probe_range(&lines);
    let dels: Vec<usize> = (b..=e).filter(|&i| is_redist_delete(&lines[i])).collect();
    assert!(
        dels.len() == 1,
        "FEMQA-VCREDIST block must contain exactly ONE Delete of vc_redist.x64.exe, found {} at lines {:?}",
        dels.len(),
        dels.iter().map(|i| i + 1).collect::<Vec<_>>()
    );
    let d = dels[0];
    assert!(
        d > pb && d < pe,
        "the single Delete (line {}) must lie inside the POSTPROBE region (lines {}-{})",
        d + 1,
        pb + 1,
        pe + 1
    );
    let last_check = (pb..pe)
        .filter(|&i| {
            let l = lines[i].to_lowercase();
            lines[i].contains("Bld") || DLLS.iter().any(|n| l.contains(n))
        })
        .max();
    assert!(
        last_check.is_some(),
        "POSTPROBE region has no line mentioning Bld or the runtime DLLs"
    );
    assert!(
        d > last_check.unwrap(),
        "Delete (line {}) must come AFTER every Bld/DLL check line (last at line {})",
        d + 1,
        last_check.unwrap() + 1
    );
}

#[test]
fn delete_is_guarded_by_if() {
    let lines = hook_lines();
    let (pb, pe) = probe_range(&lines);
    let d = (pb..pe).find(|&i| is_redist_delete(&lines[i]));
    assert!(
        d.is_some(),
        "no Delete of vc_redist.x64.exe inside the POSTPROBE region"
    );
    let d = d.unwrap();
    let cond = (pb..d).rev().find(|&i| {
        let t = lines[i].trim();
        t.starts_with("${If}")
            || t.starts_with("${IfNot}")
            || t.starts_with("${AndIf")
            || t.starts_with("${OrIf")
            || t.starts_with("${ElseIf")
            || t.starts_with("${Else}")
    });
    assert!(
        cond.is_some(),
        "Delete (line {}) is not preceded by any LogicLib conditional inside POSTPROBE",
        d + 1
    );
    let t = lines[cond.unwrap()].trim();
    assert!(
        t.starts_with("${If}"),
        "closest preceding conditional before the Delete must start with `${{If}}`, got line {}: '{t}'",
        cond.unwrap() + 1
    );
}

#[test]
fn no_redist_delete_outside_block() {
    let lines = hook_lines();
    let (b, e) = block_range(&lines);
    let stray: Vec<usize> = (0..lines.len())
        .filter(|&i| (i < b || i > e) && is_redist_delete(&lines[i]))
        .map(|i| i + 1)
        .collect();
    assert!(
        stray.is_empty(),
        "Delete of vc_redist.x64.exe outside the FEMQA-VCREDIST block at lines {stray:?} (uninstall must stay untouched)"
    );
}
