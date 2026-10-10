//! The card text must not tell the user to weaken Windows protection or
//! to allow the blocked file. It may only name the blocker, say reinstalling
//! will not help, and point at Windows Security.

use super::*;
use std::path::Path;

const FORBIDDEN: [&str; 13] = [
    "disable",
    "turn off",
    "smart app control off",
    "exclusion",
    "allow rule",
    "allowlist",
    "exclude",
    "add an exception",
    "unblock",
    "whitelist",
    "bypass",
    "run anyway",
    "allow ",
];

fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn file_name(p: &str) -> String {
    p.rsplit(['\\', '/']).next().unwrap().to_string()
}

fn check(p: &str) {
    let msg = user_message(Path::new(p));
    let norm = normalize(&msg);
    for word in FORBIDDEN {
        assert!(
            !norm.contains(word),
            "forbidden advice {word:?} in message for {p:?}: {msg}"
        );
    }
    let allow_name = format!("allow {}", file_name(p)).to_lowercase();
    assert!(
        !norm.contains(&allow_name),
        "forbidden advice {allow_name:?} in message for {p:?}: {msg}"
    );
    assert!(
        msg.starts_with(AWAITING_ADMIN_MARKER),
        "missing marker for {p:?}: {msg}"
    );
    assert!(
        norm.contains("smart app control") || norm.contains("app control"),
        "does not name the blocker for {p:?}: {msg}"
    );
    assert!(
        norm.contains("reinstalling will not help"),
        "missing 'reinstalling will not help' for {p:?}: {msg}"
    );
    assert!(
        norm.contains("windows security"),
        "missing 'Windows Security' for {p:?}: {msg}"
    );
}

#[test]
fn sac_advice_drive_path_exe() {
    check(r"C:\Users\User\AppData\Roaming\FryEdgeMiner\partners\titan\titan-edge.exe");
}

#[test]
fn sac_advice_drive_path_dll() {
    check(r"C:\Users\User\AppData\Roaming\FryEdgeMiner\partners\titan\goworkerd.dll");
}

#[test]
fn sac_advice_nt_device_path() {
    check(
        r"\Device\HarddiskVolume3\Users\x\AppData\Roaming\FryEdgeMiner\partners\titan\goworkerd.dll",
    );
}

#[test]
fn sac_advice_forward_slash_path() {
    check("C:/Users/User/AppData/Roaming/FryEdgeMiner/partners/titan/titan-edge.exe");
}

#[test]
fn sac_advice_bare_file_name() {
    check("goworkerd.dll");
}
