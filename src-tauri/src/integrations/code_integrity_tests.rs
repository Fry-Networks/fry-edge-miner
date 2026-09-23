//! B15 defect 5.
//!
//! FIXTURE PROVENANCE — READ BEFORE TRUSTING THIS FILE. The XML below is built
//! to the documented shape of a Microsoft-Windows-CodeIntegrity/Operational
//! event (the standard `<Event><System><EventID>` envelope, with the blocked
//! image reported as an NT device path). It has NOT yet been replaced by a
//! capture from a machine that really refused the DLL — B15's vm_check 4a is
//! what produces that, and the parser must be re-run against it before this
//! item is called done. The matcher is deliberately written to survive the one
//! thing a fabricated fixture is most likely to get wrong: it keys on the FILE
//! NAME, not the full path, because the event log reports
//! `\Device\HarddiskVolumeN\...` and FEM knows the file by a drive letter.

use super::*;
use std::path::{Path, PathBuf};

fn goworkerd() -> PathBuf {
    PathBuf::from(r"C:\Users\User\AppData\Roaming\FryEdgeMiner\partners\titan\goworkerd.dll")
}

const BLOCKED_GOWORKERD: &str = r#"<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'>
  <System>
    <Provider Name='Microsoft-Windows-CodeIntegrity' Guid='{4EE76BD8-3CF4-44a0-A0AC-3937643E37A3}'/>
    <EventID>3077</EventID>
    <Level>2</Level>
    <Channel>Microsoft-Windows-CodeIntegrity/Operational</Channel>
    <Computer>FEMQA-W11</Computer>
  </System>
  <EventData>
    <Data Name='File Name'>\Device\HarddiskVolume3\Users\User\AppData\Roaming\FryEdgeMiner\partners\titan\goworkerd.dll</Data>
    <Data Name='Process Name'>\Device\HarddiskVolume3\Users\User\AppData\Roaming\FryEdgeMiner\partners\titan\titan-edge.exe</Data>
    <Data Name='Requested Signing Level'>12</Data>
    <Data Name='Validated Signing Level'>0</Data>
  </EventData>
</Event>"#;

const BLOCKED_SOMETHING_ELSE: &str = r#"<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'>
  <System>
    <Provider Name='Microsoft-Windows-CodeIntegrity' Guid='{4EE76BD8-3CF4-44a0-A0AC-3937643E37A3}'/>
    <EventID>3077</EventID>
    <Channel>Microsoft-Windows-CodeIntegrity/Operational</Channel>
  </System>
  <EventData>
    <Data Name='File Name'>\Device\HarddiskVolume3\Users\User\Downloads\some-other-tool.dll</Data>
  </EventData>
</Event>"#;

#[test]
fn a_3077_event_for_our_image_is_recognised() {
    assert!(
        event_names_our_image(BLOCKED_GOWORKERD, &goworkerd()),
        "an NT device path must still match the file FEM knows by drive letter"
    );
}

#[test]
fn an_unrelated_3077_event_is_ignored() {
    assert!(!event_names_our_image(BLOCKED_SOMETHING_ELSE, &goworkerd()));
}

#[test]
fn an_event_that_is_not_a_block_is_ignored() {
    let informational =
        BLOCKED_GOWORKERD.replace("<EventID>3077</EventID>", "<EventID>3099</EventID>");
    assert!(
        !event_names_our_image(&informational, &goworkerd()),
        "only the refusal ids count — every other CodeIntegrity event is noise"
    );
}

/// Calls the PRODUCTION entry point, `first_block_for_at`. It used to call a
/// name-only `first_block_for`, which survived solely so this test did not have
/// to move — under an `#[allow(dead_code)]`, with a name that read like the
/// production path while skipping the recency check. That is a footgun, and the
/// reason it existed was this file, so this file is the right place to remove
/// the need for it.
///
/// `now` is fixed rather than `Utc::now()` so nothing here depends on the wall
/// clock. The fixtures in this file carry no `<TimeCreated>` on purpose: these
/// tests pin NAME matching, and an undated event is treated as recent, so the
/// `now` and window values below cannot change the outcome. Recency itself is
/// pinned in `code_integrity_recency_tests`, against fixtures that do carry
/// timestamps.
#[test]
fn the_first_block_for_our_image_is_picked_out_of_a_listing() {
    let listing = format!("{BLOCKED_SOMETHING_ELSE}\n\n{BLOCKED_GOWORKERD}\n\n");
    let now = chrono::DateTime::parse_from_rfc3339("2026-09-22T18:30:45Z")
        .expect("fixed clock")
        .with_timezone(&chrono::Utc);

    let found = first_block_for_at(&listing, &goworkerd(), now, BLOCK_RECENCY)
        .expect("our block must be found");
    assert!(found.contains("goworkerd.dll"));
    assert!(
        first_block_for_at(&listing, Path::new("nothing-here.dll"), now, BLOCK_RECENCY).is_none()
    );
}

#[test]
fn the_probe_script_asks_only_for_the_refusal_events() {
    let script = recent_blocks_script();
    for id in ["3033", "3076", "3077"] {
        assert!(script.contains(id), "the probe must ask for {id}: {script}");
    }
    assert!(
        script.contains("Microsoft-Windows-CodeIntegrity/Operational"),
        "the probe must read the CodeIntegrity channel: {script}"
    );
    assert!(
        script.contains("SilentlyContinue"),
        "a disabled channel is not an error condition for FEM: {script}"
    );
}

/// THE CROSS-MODULE PROOF, and the whole point of the item: the message this
/// module produces must stop the restart loop. Calls the EXISTING pure
/// `recovery_action` without editing health.rs or any of its tests.
#[test]
fn a_code_integrity_block_stops_the_respawn_loop() {
    use crate::integrations::HealthStatus;
    use crate::supervisor::health::{recovery_action, RecoveryAction};

    let reason = user_message(&goworkerd());
    assert_eq!(
        recovery_action(&HealthStatus::Unhealthy(reason), true, 0, 6),
        RecoveryAction::None,
        "an OS refusal is not something a restart can fix — respawning through \
         it every ~5 minutes forever is the defect"
    );
}

/// Non-vacuity: an ordinary crash must still restart.
#[test]
fn an_ordinary_crash_still_restarts() {
    use crate::integrations::HealthStatus;
    use crate::supervisor::health::{recovery_action, RecoveryAction};

    assert_eq!(
        recovery_action(
            &HealthStatus::Unhealthy("titan-edge process is not running".to_string()),
            true,
            0,
            6
        ),
        RecoveryAction::Restart
    );
}

/// The message has to be actionable, and has to say the one thing that stops a
/// user from wasting an afternoon: this is not a corrupted download.
#[test]
fn the_message_names_the_file_and_rules_out_a_reinstall() {
    let msg = user_message(&goworkerd());
    assert!(
        msg.starts_with(AWAITING_ADMIN_MARKER),
        "the marker must lead, or awaits_user_action does not match it: {msg}"
    );
    assert!(msg.contains("goworkerd.dll"), "{msg}");
    assert!(
        msg.contains("reinstalling will not help"),
        "a pinned sha256 PASSES on a file SAC refuses — say so: {msg}"
    );
    assert!(
        msg.contains("Windows Security") || msg.contains("Smart App Control"),
        "the user needs to be told where to allow it: {msg}"
    );
}
