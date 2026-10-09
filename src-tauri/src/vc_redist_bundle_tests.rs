//! RED: the installer must bundle the Microsoft-signed VC++ x64 redistributable
//! and install it (bounded, interactive-only, never failing the install).
//!
//! Files under test are read at RUNTIME so a missing file is an assertion
//! failure, not a compile error.

use std::path::{Path, PathBuf};

const PIN_SHA256: &str = "cc0ff0eb1dc3f5188ae6300faef32bf5beeba4bdd6e8e445a9184072096b713b";
const PIN_URL: &str = "https://download.visualstudio.microsoft.com/download/pr/bd1c8d9d-ba95-4eee-bc6e-df1fcc876373/CC0FF0EB1DC3F5188AE6300FAEF32BF5BEEBA4BDD6E8E445A9184072096B713B/VC_redist.x64.exe";
const BEGIN: &str = "; >>> FEMQA-VCREDIST";
const END: &str = "; <<< FEMQA-VCREDIST";

fn manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn repo() -> PathBuf {
    manifest().join("..")
}

fn read(p: PathBuf, what: &str) -> String {
    assert!(p.exists(), "{what} is missing: {}", p.display());
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read {what}: {e}"))
}

fn hook() -> String {
    read(
        manifest().join("nsis-hooks.nsh"),
        "src-tauri/nsis-hooks.nsh",
    )
}

/// Text between the FEMQA-VCREDIST markers; asserts markers and that the
/// block lies inside NSIS_HOOK_POSTINSTALL.
fn block_of(h: &str) -> String {
    assert!(
        h.contains(BEGIN),
        "marker '{BEGIN}' missing from nsis-hooks.nsh"
    );
    assert!(
        h.contains(END),
        "marker '{END}' missing from nsis-hooks.nsh"
    );
    let b = h.find(BEGIN).unwrap();
    let e = h.find(END).unwrap();
    assert!(e > b, "end marker precedes begin marker");
    let m = h
        .find("!macro NSIS_HOOK_POSTINSTALL")
        .expect("!macro NSIS_HOOK_POSTINSTALL missing");
    let me = h[m..]
        .find("!macroend")
        .map(|i| i + m)
        .expect("!macroend after NSIS_HOOK_POSTINSTALL missing");
    assert!(
        b > m && e < me,
        "FEMQA-VCREDIST block is not inside the NSIS_HOOK_POSTINSTALL macro"
    );
    h[b..e].to_string()
}

fn has(hay: &str, needle: &str, what: &str) {
    assert!(hay.contains(needle), "missing {what}: '{needle}'");
}

fn has_ci(hay: &str, needle: &str, what: &str) {
    assert!(
        hay.to_lowercase().contains(&needle.to_lowercase()),
        "missing {what}: '{needle}' (case-insensitive)"
    );
}

#[test]
fn windows_conf_bundles_redist() {
    let s = read(
        manifest().join("tauri.windows.conf.json"),
        "src-tauri/tauri.windows.conf.json",
    );
    let v: serde_json::Value =
        serde_json::from_str(&s).expect("tauri.windows.conf.json must parse");
    let arr = v["bundle"]["resources"].as_array();
    assert!(
        arr.is_some(),
        "bundle.resources must be an array in tauri.windows.conf.json"
    );
    let res = arr.unwrap();
    for want in ["resources/vc_redist.x64.exe", "resources/frynode.exe"] {
        assert!(
            res.iter().any(|x| x.as_str() == Some(want)),
            "bundle.resources lacks '{want}'"
        );
    }
}

#[test]
fn base_conf_unchanged_resources() {
    let s = read(
        manifest().join("tauri.conf.json"),
        "src-tauri/tauri.conf.json",
    );
    let v: serde_json::Value = serde_json::from_str(&s).expect("tauri.conf.json must parse");
    let res = v["bundle"]["resources"]
        .as_array()
        .expect("bundle.resources array");
    let got: Vec<&str> = res.iter().filter_map(|x| x.as_str()).collect();
    assert_eq!(got, vec!["resources/frynode.exe"], "base resources changed");
    assert_eq!(v["bundle"]["windows"]["nsis"]["installMode"], "currentUser");
    assert_eq!(
        v["bundle"]["windows"]["nsis"]["installerHooks"],
        "nsis-hooks.nsh"
    );
}

#[test]
fn prebuild_runs_fetch() {
    let s = read(repo().join("package.json"), "package.json");
    let v: serde_json::Value = serde_json::from_str(&s).expect("package.json must parse");
    let prebuild = v["scripts"]["prebuild"].as_str();
    assert!(
        prebuild.is_some(),
        "scripts.prebuild missing in package.json"
    );
    let pre = prebuild.unwrap();
    has(pre, "scripts/fetch-vcredist.mjs", "prebuild fetch script");
    assert_eq!(
        v["scripts"]["build"].as_str(),
        Some("tsc && vite build"),
        "scripts.build changed"
    );
}

#[test]
fn node_wrapper_noop_off_windows() {
    let s = read(
        repo().join("scripts/fetch-vcredist.mjs"),
        "scripts/fetch-vcredist.mjs",
    );
    for n in [
        "process.platform",
        "win32",
        "powershell",
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        "fetch-vcredist.ps1",
        "process.exit",
    ] {
        has(&s, n, "wrapper element");
    }
}

#[test]
fn wrapper_strips_psmodulepath_for_windows_powershell() {
    let s = read(
        repo().join("scripts/fetch-vcredist.mjs"),
        "scripts/fetch-vcredist.mjs",
    );
    has_ci(&s, "PSModulePath", "PSModulePath handling");
    let i = s.find("spawnSync(").expect("spawnSync call");
    let call = &s[i..];
    assert!(
        call.contains("env:") || call.contains("env,"),
        "env not passed to spawnSync"
    );
}

#[test]
fn ps1_pins_and_verifies() {
    let s = read(
        repo().join("scripts/fetch-vcredist.ps1"),
        "scripts/fetch-vcredist.ps1",
    );
    has(&s, PIN_URL, "pinned URL");
    has_ci(&s, PIN_SHA256, "pinned SHA256");
    for n in [
        "Get-FileHash",
        "SHA256",
        "Get-AuthenticodeSignature",
        "Valid",
        "Microsoft Corporation",
        "Microsoft Root Certificate Authority",
        "exit 1",
        "Test-Path",
    ] {
        has(&s, n, "verification element");
    }
    assert!(
        s.contains("src-tauri/resources/vc_redist.x64.exe")
            || s.contains("src-tauri\\resources\\vc_redist.x64.exe"),
        "missing target path src-tauri/resources/vc_redist.x64.exe"
    );
    assert!(
        !s.contains("aka.ms"),
        "must not use aka.ms (pinned immutable URL only)"
    );
}

#[test]
fn gitignore_excludes_redist() {
    let s = read(repo().join(".gitignore"), ".gitignore");
    assert!(
        s.lines()
            .any(|l| l.trim() == "src-tauri/resources/vc_redist.x64.exe"),
        ".gitignore lacks line 'src-tauri/resources/vc_redist.x64.exe'"
    );
}

/// Integer literals on a line; tokens right after '$' (NSIS registers) skipped.
fn ints(line: &str) -> Vec<String> {
    let c: Vec<char> = line.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < c.len() {
        if c[i].is_ascii_alphanumeric()
            && (i == 0 || !(c[i - 1].is_ascii_alphanumeric() || c[i - 1] == '_' || c[i - 1] == '$'))
        {
            let st = i;
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let t: String = c[st..i].iter().collect();
            if t.chars().next().unwrap().is_ascii_digit() {
                out.push(t);
            }
        } else {
            i += 1;
        }
    }
    out
}

#[test]
fn nsis_hook_redist_block() {
    let h = hook();
    let b = block_of(&h);
    has(&b, "SetRegView 64", "SetRegView 64");
    assert!(
        b.contains("SetRegView lastused") || b.contains("SetRegView 32"),
        "missing SetRegView restore (lastused or 32)"
    );
    has(&b, "${DisableX64FSRedirection}", "DisableX64FSRedirection");
    has(&b, "${EnableX64FSRedirection}", "EnableX64FSRedirection");
    has(
        &b,
        "SOFTWARE\\Microsoft\\VisualStudio\\14.0\\VC\\Runtimes\\x64",
        "runtime reg key",
    );
    for n in ["Installed", "Bld", "35211"] {
        has(&b, n, "registry/build check");
    }
    for n in [
        "vcruntime140.dll",
        "vcruntime140_1.dll",
        "msvcp140.dll",
        "msvcp140_atomic_wait.dll",
    ] {
        has_ci(&b, n, "DLL presence check");
    }
    has(&b, "$INSTDIR\\resources\\vc_redist.x64.exe", "redist path");
    for n in [
        "CreateProcessW",
        "WaitForSingleObject",
        "GetExitCodeProcess",
        "CloseHandle",
    ] {
        has(&b, n, "process API");
    }
    let mut all = vec![];
    for l in b.lines().filter(|l| l.contains("WaitForSingleObject")) {
        assert!(
            !l.to_uppercase().contains("INFINITE"),
            "WaitForSingleObject must not be INFINITE"
        );
        all.extend(ints(l));
    }
    let vals: Vec<u64> = all
        .iter()
        .map(
            |t| match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                Some(x) => u64::from_str_radix(x, 16).unwrap_or(u64::MAX),
                None => t.parse::<u64>().unwrap_or(u64::MAX),
            },
        )
        .collect();
    assert!(
        vals.iter().any(|v| *v > 0 && *v <= 120000),
        "WaitForSingleObject needs a finite timeout <= 120000 ms; found {all:?}"
    );
    assert!(
        !vals.contains(&0xFFFF_FFFF),
        "WaitForSingleObject uses 0xFFFFFFFF (infinite)"
    );
    assert!(
        !b.contains("-1)") && !b.contains("i -1"),
        "WaitForSingleObject must not use -1"
    );
    has(&b, "/install /quiet /norestart", "installer switches");
    for n in ["1638", "3010", "$PassiveMode", "$UpdateMode", "DetailPrint"] {
        has(&b, n, "exit-code/gating element");
    }
    assert!(
        b.contains("${Silent}") || b.contains("IfSilent"),
        "missing silent gating (${{Silent}} or IfSilent)"
    );
    let lb = b.to_lowercase();
    for bad in [
        "execwait",
        "execshell",
        "nsexec",
        "runas",
        "powershell",
        "cmd.exe",
        "wscript",
        "$pluginsdir",
        "abort",
        "quit",
    ] {
        assert!(
            !lb.contains(bad),
            "forbidden token in redist block: '{bad}'"
        );
    }
}

#[test]
fn uninstall_untouched() {
    let h = hook();
    let b = block_of(&h);
    let pos = h.find(BEGIN).unwrap();
    for m in [
        "!macro NSIS_HOOK_PREUNINSTALL",
        "!macro NSIS_HOOK_POSTUNINSTALL",
    ] {
        if let Some(s) = h.find(m) {
            let e = h[s..].find("!macroend").map(|i| i + s).unwrap_or(h.len());
            assert!(!(pos > s && pos < e), "redist block lies inside {m}");
        }
    }
    let outside = h.replace(&b, "");
    for (n, l) in outside.lines().enumerate() {
        assert!(
            !l.to_lowercase().contains("vc_redist"),
            "vc_redist referenced outside block at line {}: {l}",
            n + 1
        );
    }
}

#[test]
fn existing_quoting_tripwire_compatible() {
    let b = block_of(&hook());
    assert!(
        !b.contains("Get-CimInstance"),
        "block must not contain Get-CimInstance"
    );
}
