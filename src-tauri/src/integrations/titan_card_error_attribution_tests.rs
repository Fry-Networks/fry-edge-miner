//! D1 — the Titan card blames a missing VC++ runtime for start failures that
//! have a different cause (App Control os error 4551, other startup exits).
//! The classifier is pure, so these run on any platform.

use super::*;

const VC_LINK: &str = "https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist";
const TAIL: &str = "no error output";

fn vc_text(m: &str) -> bool {
    let u = m.to_uppercase();
    u.contains("VC++") || u.contains("VCRUNTIME") || u.contains("MSVCP")
}

fn assert_no_security_advice(m: &str) {
    let l = m.to_lowercase();
    for w in [
        "disable",
        "turn off",
        "exclusion",
        "allow rule",
        "allowlist",
    ] {
        assert!(!l.contains(w), "message advises '{w}': {m}");
    }
}

fn assert_generic(m: &str) {
    let l = m.to_lowercase();
    assert!(l.contains("titan"), "must name the component: {m}");
    assert!(!vc_text(m), "must not show VC++ text: {m}");
    for raw in [
        "0xc0000142",
        "0xc000012d",
        "3221225794",
        "3221225773",
        "-1073741502",
        "-1073741619",
        "c0000142",
        "c000012d",
    ] {
        assert!(!l.contains(raw), "raw code '{raw}' leaked: {m}");
    }
    assert_no_security_advice(m);
}

#[test]
fn spawn_os_error_4551_is_app_control_not_vc_redist() {
    let m = classify_not_running(Some(StartFailure::SpawnOsError(4551)), true, TAIL);
    assert!(
        m.contains("blocked by Windows Application Control"),
        "expected App Control text: {m}"
    );
    assert!(!vc_text(&m), "App Control block shown as VC++ problem: {m}");
    assert!(!m.contains("4551"), "raw code leaked: {m}");
    assert_no_security_advice(&m);
}

#[test]
fn child_exit_c0000135_is_vc_redist_with_link() {
    let m = classify_not_running(
        Some(StartFailure::ChildExitStatus(0xC000_0135)),
        false,
        TAIL,
    );
    assert!(vc_text(&m), "expected VC++ runtime text: {m}");
    assert!(m.contains(VC_LINK), "missing VC++ download link: {m}");
    assert_no_security_advice(&m);
}

#[test]
fn child_exit_c0000142_is_generic_humanized() {
    assert_generic(&classify_not_running(
        Some(StartFailure::ChildExitStatus(0xC000_0142)),
        true,
        TAIL,
    ));
}

#[test]
fn child_exit_c000012d_is_generic_humanized() {
    assert_generic(&classify_not_running(
        Some(StartFailure::ChildExitStatus(0xC000_012D)),
        true,
        TAIL,
    ));
}

#[test]
fn child_exit_other_code_is_generic_humanized() {
    assert_generic(&classify_not_running(
        Some(StartFailure::ChildExitStatus(0xC000_0409)),
        true,
        TAIL,
    ));
}

#[test]
fn no_recorded_failure_keeps_todays_text() {
    for tail in [TAIL, "panic: disk full"] {
        for vc in [true, false] {
            assert_eq!(
                classify_not_running(None, vc, tail),
                process_not_running_reason(vc, tail)
            );
        }
    }
    assert!(vc_text(&classify_not_running(None, true, TAIL)));
}
