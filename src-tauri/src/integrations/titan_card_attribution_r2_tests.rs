//! D1 round 2: App Control siblings, card selection order, and that
//! `health_check` really goes through the selector. Pure, runs on any platform.

use super::*;
use crate::integrations::code_integrity::AWAITING_ADMIN_MARKER;

const TAIL: &str = "no error output";
const BLOCK: &str = "BLOCK-TEXT-FROM-RECENT_BLOCK";
const TITAN_SRC: &str = include_str!("titan.rs");

fn vc_text(m: &str) -> bool {
    let u = m.to_uppercase();
    u.contains("VC++") || u.contains("VCRUNTIME") || u.contains("MSVCP")
}

fn assert_app_control(m: &str) {
    assert!(
        m.contains("blocked by Windows Application Control"),
        "expected App Control text: {m}"
    );
    assert!(
        m.starts_with(AWAITING_ADMIN_MARKER),
        "marker must lead: {m}"
    );
    assert!(!vc_text(m), "App Control block shown as VC++: {m}");
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
    assert!(m.to_lowercase().contains("titan"), "must name titan: {m}");
    assert!(!vc_text(m), "VC++ text on a generic failure: {m}");
    assert!(!m.starts_with(AWAITING_ADMIN_MARKER), "no marker: {m}");
}

#[test]
fn app_control_spawn_errors_are_app_control() {
    for code in [4551, 577, 1260] {
        let m = classify_not_running(Some(StartFailure::SpawnOsError(code)), true, TAIL);
        assert_app_control(&m);
        assert!(!m.contains(&code.to_string()), "raw code leaked: {m}");
    }
}

#[test]
fn app_control_exit_statuses_are_app_control() {
    for code in [0xC0E9_0002u32, 0xC000_0428] {
        let m = classify_not_running(Some(StartFailure::ChildExitStatus(code)), true, TAIL);
        assert_app_control(&m);
    }
}

#[test]
fn neighbouring_codes_are_not_app_control() {
    for code in [4552, 4553, 1261] {
        let m = classify_not_running(Some(StartFailure::SpawnOsError(code)), false, TAIL);
        assert_generic(&m);
    }
}

#[test]
fn known_codes_decide_alone_even_when_a_block_is_found() {
    let ac = select_not_running_reason(
        Some(StartFailure::SpawnOsError(4551)),
        Some(BLOCK.to_string()),
        true,
        TAIL,
    );
    assert_app_control(&ac);
    let vc = select_not_running_reason(
        Some(StartFailure::ChildExitStatus(0xC000_0135)),
        Some(BLOCK.to_string()),
        false,
        TAIL,
    );
    assert!(vc_text(&vc) && !vc.contains(BLOCK), "{vc}");
}

#[test]
fn another_recorded_code_consults_the_block_first() {
    let m = select_not_running_reason(
        Some(StartFailure::ChildExitStatus(0xC000_0409)),
        Some(BLOCK.to_string()),
        true,
        TAIL,
    );
    assert_eq!(m, BLOCK);
}

#[test]
fn another_recorded_code_without_a_block_is_generic() {
    assert_generic(&select_not_running_reason(
        Some(StartFailure::ChildExitStatus(0xC000_0409)),
        None,
        true,
        TAIL,
    ));
}

#[test]
fn no_recorded_code_uses_the_block_else_todays_heuristic() {
    assert_eq!(
        select_not_running_reason(None, Some(BLOCK.to_string()), true, TAIL),
        BLOCK
    );
    for tail in [TAIL, "panic: disk full"] {
        for vc in [true, false] {
            assert_eq!(
                select_not_running_reason(None, None, vc, tail),
                process_not_running_reason(vc, tail)
            );
        }
    }
}

#[test]
fn health_check_goes_through_the_selector() {
    let at = TITAN_SRC
        .find("async fn health_check(&self)")
        .expect("health_check present");
    let rest = &TITAN_SRC[at..];
    let end = rest
        .find("// Read both log files")
        .expect("end of the dead-process branch");
    let body = &rest[..end];
    assert!(
        body.contains("select_not_running_reason("),
        "health_check must call the selector"
    );
    assert!(
        !body.contains("classify_not_running("),
        "health_check must not bypass the selector"
    );
}
