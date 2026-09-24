//! NB-2: a timed-out VC++ redist install was reported as a DECLINE. The gate
//! classifies every TimedOut closure error as declined and replaces it with
//! NEEDS_APPROVAL_MESSAGE, so titan's own "timeout means still installing"
//! mapping could never see it. The real install cannot run on a test host (it
//! downloads from aka.ms and elevates), so the first two tests read the real
//! source; the `real_*` tests drive the real gate with a real timed-out child.

use crate::elevation_gate::{
    blocked_reasons, run_elevated, ElevationSkipped, ElevationTrigger, NEEDS_APPROVAL_MESSAGE,
};
use crate::supervisor::platform::BoundedOutput;
use std::time::Duration;

const TITAN_SRC: &str = include_str!("titan.rs");

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `install_vc_redist_elevated`'s body, comments stripped.
fn install_body() -> String {
    let code = code_only(TITAN_SRC);
    let at = code
        .find(&format!("async fn install_vc_redist{}(", "_elevated"))
        .expect("install_vc_redist_elevated must exist");
    let end = code[at..]
        .find("\n}\n")
        .map(|e| at + e)
        .expect("fn must close at column 0");
    code[at..end].to_string()
}

/// The closure the REAL install hands to the gate (not the Automatic refusal).
fn install_closure(body: &str) -> &str {
    let at = body
        .find(&format!("run_elevated(\"titan\", &attempt{}", "_key"))
        .expect("the real install must be gated with its attempt key");
    let end = body[at..]
        .find("\n    });")
        .map(|e| at + e)
        .expect("closure must close");
    &body[at..end]
}

#[test]
fn the_install_timeout_never_enters_the_gates_error_channel() {
    let body = install_body();
    let closure = install_closure(&body);
    // Positive controls: the right closure, and the downstream mapping.
    assert!(
        closure.contains("output_bounded(VC_REDIST_INSTALL_TIMEOUT)"),
        "control: wrong closure:\n{closure}"
    );
    assert!(
        body.contains("vc_redist_install_outcome(true"),
        "control: TimedOut must still map to StillInstalling"
    );
    let timed_out = format!("ErrorKind::Timed{}", "Out");
    assert!(
        closure.contains(&timed_out),
        "NB-2: the VC++ redist install closure hands output_bounded's TimedOut to the \
         elevation gate, which classifies it as declined and replaces it with the \
         needs-approval message, so a user who APPROVED an install that outlived the \
         budget is told to approve again:\n{closure}"
    );
    let arm = &closure[closure.find(&timed_out).unwrap()..];
    let expr = arm[arm.find("=>").expect("TimedOut must be a match arm") + 2..].trim_start();
    assert!(
        expr.starts_with("Ok("),
        "NB-2: the TimedOut arm must hand the timeout back INSIDE Ok so the gate never \
         classifies it:\n{closure}"
    );
}

/// GUARD (green before and after the fix): whatever the gate hands back on
/// its Ok side must reach the TimedOut -> StillInstalling mapping below it.
/// Kills the mutant that carries the timeout through the gate and then maps
/// it to a failure in the `Ok` arm.
#[test]
fn the_gates_ok_side_is_passed_through_to_the_still_installing_mapping() {
    let body = install_body();
    let m = body
        .find("match gated {")
        .expect("control: the gate's result must be matched");
    let rest = &body[m..];
    let ok_at = rest.find("Ok(").expect("control: an Ok arm");
    let ok_arm = &rest[ok_at..];
    let ok_arm = &ok_arm[..ok_arm
        .find("\n        Err(")
        .expect("control: an Err arm follows the Ok arm")];
    assert!(
        !ok_arm.contains("return") && !ok_arm.contains("vc_redist_install_outcome(false"),
        "NB-2: the gate's Ok side is turned into a failure before it can reach the \
         TimedOut -> StillInstalling mapping:\n{ok_arm}"
    );
}

#[test]
fn the_timeout_is_not_recovered_by_reading_the_gates_message() {
    let body = install_body();
    assert!(body.contains("run_elevated("), "control: body not found");
    let sniff = format!("reason.con{}(", "tains");
    assert!(
        !body.contains(&sniff),
        "NB-2: install_vc_redist_elevated recovers the io kind by string-matching the \
         gate's reason, which the gate has already rewritten to the needs-approval message"
    );
}

fn hung_child() -> std::process::Command {
    if cfg!(target_os = "windows") {
        let mut c = crate::supervisor::platform::command("powershell");
        c.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"]);
        c
    } else {
        let mut c = crate::supervisor::platform::command("sleep");
        c.arg("30");
        c
    }
}

/// SUPPORTING (green before and after the fix): why a text check on the
/// gate's reason could never see a timeout.
#[test]
fn real_gate_rewrites_a_timed_out_closure_error_so_its_text_cannot_survive() {
    let gated = run_elevated(
        "test-nb2-premise",
        "nb2|premise",
        ElevationTrigger::UserClick,
        || {
            hung_child()
                .output_bounded(Duration::from_secs(1))
                .map_err(anyhow::Error::new)
        },
    );
    match gated {
        Err(ElevationSkipped::Failed(reason)) => {
            assert_eq!(reason, NEEDS_APPROVAL_MESSAGE);
            assert!(!reason.contains("timed out") && !reason.contains("TimedOut"));
        }
        other => panic!("expected a real TimedOut to be classified as a decline, got {other:?}"),
    }
    assert_eq!(
        blocked_reasons()
            .get("test-nb2-premise")
            .map(String::as_str),
        Some(NEEDS_APPROVAL_MESSAGE)
    );
}

/// SUPPORTING (green before and after the fix): a timeout carried inside Ok
/// passes the REAL gate intact and clears the card, while the attempt stays
/// spent, so no second prompt appears without the retry gesture.
#[test]
fn real_gate_passes_a_timeout_carried_inside_ok_through_intact_and_clears_the_card() {
    let p = "test-nb2-carry";
    let _ = run_elevated(p, "nb2|carry-pre", ElevationTrigger::Automatic, || Ok(()));
    assert!(
        blocked_reasons().contains_key(p),
        "control: a block must be on the card first"
    );
    let gated = run_elevated(
        p,
        "nb2|carry",
        ElevationTrigger::UserClick,
        || match hung_child().output_bounded(Duration::from_secs(1)) {
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Ok(Err(e)),
            other => other.map(Ok).map_err(anyhow::Error::new),
        },
    );
    let inner = gated.expect("a timeout carried inside Ok is not a gate failure");
    assert_eq!(
        inner.expect_err("the child outlived its budget").kind(),
        std::io::ErrorKind::TimedOut
    );
    assert_eq!(
        blocked_reasons().get(p),
        None,
        "an approving user must not be told to approve again"
    );
    let again = run_elevated(
        p,
        "nb2|carry",
        ElevationTrigger::UserClick,
        || -> anyhow::Result<()> { panic!("must not raise a second prompt") },
    );
    assert_eq!(again, Err(ElevationSkipped::AlreadyAttempted));
}
