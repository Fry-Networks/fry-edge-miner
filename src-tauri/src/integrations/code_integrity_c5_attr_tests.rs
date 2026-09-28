//! c5 F7 pin for survivor M18b (R/Q3-CODE-LEVEL-c4.md, executed survivors).
//!
//! BUG LOOP 2 (0218d14) made `<EventID>` the only thing that decides audit
//! (3076) vs. enforced block (3033/3077). The id-anywhere matcher it replaced
//! had two halves: element text (`>3077<`) and attribute values (`'3077'`).
//! `code_integrity_b15_capture_tests.rs` pins the element half with decoys
//! (EventRecordID, USN) but no attribute decoy, so re-adding only the
//! attribute half (M18b) passed the whole suite. Yet every real event carries
//! attribute values FEM does not control: `<Execution ProcessID='..'
//! ThreadID='..'/>`. An audit event from a process whose id happens to be
//! 3077 would read as an enforced block: "Awaiting administrator action"
//! and recovery suppressed for a process that is running.

use super::*;
use std::path::PathBuf;

fn titan_edge() -> PathBuf {
    PathBuf::from(r"C:\Users\femqa\AppData\Local\Fry Edge Miner\partners\titan\titan-edge.exe")
}

/// Verbatim copy of `CAPTURED_TITAN_EDGE_AUDIT` in
/// `code_integrity_b15_capture_tests.rs` (EventRecordID 75 of the B15 VM
/// leg's real capture, an AUDIT-mode 3076 for titan-edge.exe; lab artifacts,
/// nothing secret). Copied because that constant is private to its module.
const CAPTURED_TITAN_EDGE_AUDIT: &str = r#"<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='Microsoft-Windows-CodeIntegrity' Guid='{4ee76bd8-3cf4-44a0-a0ac-3937643e37a3}'/><EventID>3076</EventID><Version>5</Version><Level>4</Level><Task>18</Task><Opcode>118</Opcode><Keywords>0x8000000000000000</Keywords><TimeCreated SystemTime='2026-09-24T17:39:17.1330993Z'/><EventRecordID>75</EventRecordID><Correlation ActivityID='{a0081f4c-4bd7-0000-b9d1-0da0d74bdd01}'/><Execution ProcessID='2808' ThreadID='8112'/><Channel>Microsoft-Windows-CodeIntegrity/Operational</Channel><Computer>FEMQA-W11</Computer><Security UserID='S-1-5-21-2865687314-2573020260-3100654925-1000'/></System><EventData><Data Name='FileNameLength'>78</Data><Data Name='File Name'>\Device\HarddiskVolume6\fry_storage\FryEdgeMiner\partners\titan\titan-edge.exe</Data><Data Name='ProcessNameLength'>83</Data><Data Name='Process Name'>\Device\HarddiskVolume3\Users\femqa\AppData\Local\Fry Edge Miner\fry-edge-miner.exe</Data><Data Name='Requested Signing Level'>2</Data><Data Name='Validated Signing Level'>2</Data><Data Name='Status'>0xc0e90002</Data><Data Name='SHA1 Hash Size'>20</Data><Data Name='SHA1 Hash'>171AB5B521D37DF5D2E6E2986ED616489274EF26</Data><Data Name='SHA256 Hash Size'>32</Data><Data Name='SHA256 Hash'>E89C47AFFA5C36F8E9B81DB6567E424AEE49617CE9D02E08DC614A1296382C34</Data><Data Name='SHA1 Flat Hash Size'>20</Data><Data Name='SHA1 Flat Hash'>9A0828C82945B2BF2FF51E67CACF8952D36A3F59</Data><Data Name='SHA256 Flat Hash Size'>32</Data><Data Name='SHA256 Flat Hash'>A9E4A521343A1CE6800BA15178D7DA403CD48CCF15E3DF4ADC464969DCF95E8B</Data><Data Name='USN'>0</Data><Data Name='SI Signing Scenario'>1</Data><Data Name='PolicyNameLength'>46</Data><Data Name='PolicyName'>FEMQA-B15-AUDIT lab policy (remove after cell)</Data><Data Name='PolicyIDLength'>6</Data><Data Name='PolicyID'>022422</Data><Data Name='PolicyHashSize'>32</Data><Data Name='PolicyHash'>F109BB9689C313B627782A488D85EA773F65BFC98F5B52E26F6E06E47A8EAFC4</Data><Data Name='OriginalFileNameLength'>0</Data><Data Name='OriginalFileName'></Data><Data Name='InternalNameLength'>0</Data><Data Name='InternalName'></Data><Data Name='FileDescriptionLength'>0</Data><Data Name='FileDescription'></Data><Data Name='ProductNameLength'>0</Data><Data Name='ProductName'></Data><Data Name='FileVersion'>0.0.0.0</Data><Data Name='PolicyGUID'>{d69c1cba-e1fc-489e-b177-a9ba36e1d7e6}</Data><Data Name='UserWriteable'>false</Data><Data Name='PackageFamilyNameLength'>0</Data><Data Name='PackageFamilyName'></Data></EventData></Event>"#;

const EXECUTION: &str = "<Execution ProcessID='2808' ThreadID='8112'/>";

fn captured_now() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-09-24T17:39:25Z")
        .expect("fixed clock")
        .with_timezone(&chrono::Utc)
}

/// The real capture with only its `<Execution>` attributes replaced.
fn with_execution(process_id: &str, thread_id: &str) -> String {
    assert_eq!(
        CAPTURED_TITAN_EDGE_AUDIT.matches(EXECUTION).count(),
        1,
        "fixture sanity: the capture carries one Execution element"
    );
    let decoy = CAPTURED_TITAN_EDGE_AUDIT.replacen(
        EXECUTION,
        &format!("<Execution ProcessID='{process_id}' ThreadID='{thread_id}'/>"),
        1,
    );
    assert!(
        decoy.contains("<EventID>3076</EventID>"),
        "fixture sanity: EventID must stay 3076: {decoy}"
    );
    assert!(
        event_names_our_image(&decoy, &titan_edge()),
        "fixture sanity: the decoy must still name titan-edge.exe"
    );
    decoy
}

fn reads_as_block(event: &str) -> bool {
    let listing = format!("{event}\r\n\r\n");
    first_block_for_at(&listing, &titan_edge(), captured_now(), BLOCK_RECENCY).is_some()
}

#[test]
fn a_3076_audit_event_from_process_id_3077_is_still_not_a_block() {
    let decoy = with_execution("3077", "8112");
    assert!(
        !event_is_enforced_block(&decoy),
        "a 3076 (audit) event must not become an enforced block because its \
         ProcessID attribute happens to equal 3077; only <EventID> may decide: {decoy}"
    );
    assert!(!reads_as_block(&decoy), "{decoy}");
}

#[test]
fn a_3076_audit_event_on_thread_id_3033_is_still_not_a_block() {
    let decoy = with_execution("2808", "3033");
    assert!(
        !event_is_enforced_block(&decoy),
        "a 3076 (audit) event must not become an enforced block because its \
         ThreadID attribute happens to equal 3033; only <EventID> may decide: {decoy}"
    );
    assert!(!reads_as_block(&decoy), "{decoy}");
}

/// Control: the same decoy is a block once `<EventID>` itself says 3077, so
/// the two tests above fail for the attribute and not for a broken fixture.
#[test]
fn the_same_event_with_event_id_3077_is_a_block() {
    let enforced = with_execution("3077", "8112").replacen(
        "<EventID>3076</EventID>",
        "<EventID>3077</EventID>",
        1,
    );
    assert!(event_is_enforced_block(&enforced), "{enforced}");
    assert!(reads_as_block(&enforced), "{enforced}");
}
