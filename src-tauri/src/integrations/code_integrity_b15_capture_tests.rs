//! FAIL-9 and FAIL-10 (row 5): real captures from the B15 VM leg
//! (~/femqa/evidence/matrix-c3/c3-b15-wdac/w11/{b15-fem-script-output.txt,
//! b15-ci-events-titan.xml.txt}), copied verbatim below. Unlike the fixtures
//! in `code_integrity_tests.rs` — whose own header notes they are SYNTHETIC,
//! built to the documented event shape and "not yet ... replaced by a capture
//! from a machine that really refused the DLL" — these three events ARE that
//! capture: a machine running "FEMQA-B15-AUDIT lab policy", an AUDIT-mode
//! WDAC policy. Values here are lab artifacts (hashes of a build under test,
//! a throwaway policy name/GUID, a lab hostname); nothing is a secret.
//!
//! FAIL-9: every event below carries `<EventID>3076</EventID>` — the AUDIT
//! id. Under an Audit policy, 3076 means Code Integrity logged that the image
//! WOULD have failed an enforced policy, but the image WAS ALLOWED TO LOAD.
//! `code_integrity.rs` used to treat 3076 the same as the enforced-refusal
//! ids (3033, 3077) — and had 3076/3077's meanings swapped in its own doc
//! comment — so this exact, real, non-blocking capture made FEM report
//! "Awaiting administrator action" and permanently suppress recovery for a
//! process that was, in fact, running.
//!
//! FAIL-10: PowerShell's `ToXml()` output uses CRLF, and the real capture
//! separates events with `"\r\n\r\n"` — confirmed with `od -c` — which does
//! not contain the substring `"\n\n"` `first_block_for_at` split on. The
//! whole capture collapsed into ONE chunk, and `event_time` (which finds the
//! FIRST `SystemTime=` in whatever chunk it is given) judged every event in
//! the listing by whichever timestamp happened to come first in the raw
//! string, not the timestamp of the event that actually matched.

use super::*;
use std::path::PathBuf;

fn goworkerd() -> PathBuf {
    PathBuf::from(r"C:\Users\femqa\AppData\Local\Fry Edge Miner\partners\titan\goworkerd.dll")
}

fn titan_edge() -> PathBuf {
    PathBuf::from(r"C:\Users\femqa\AppData\Local\Fry Edge Miner\partners\titan\titan-edge.exe")
}

/// EventRecordID 77 — b15-ci-events-titan.xml.txt / b15-fem-script-output.txt,
/// first event. Audit-mode (3076): goworkerd.dll would have failed the
/// FEMQA-B15-AUDIT lab policy, but was allowed to load.
const CAPTURED_GOWORKERD_AUDIT: &str = r#"<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='Microsoft-Windows-CodeIntegrity' Guid='{4ee76bd8-3cf4-44a0-a0ac-3937643e37a3}'/><EventID>3076</EventID><Version>5</Version><Level>4</Level><Task>18</Task><Opcode>118</Opcode><Keywords>0x8000000000000000</Keywords><TimeCreated SystemTime='2026-09-24T17:39:20.0481290Z'/><EventRecordID>77</EventRecordID><Correlation ActivityID='{a0081f4c-4bd7-0003-9814-0da0d74bdd01}'/><Execution ProcessID='1776' ThreadID='8032'/><Channel>Microsoft-Windows-CodeIntegrity/Operational</Channel><Computer>FEMQA-W11</Computer><Security UserID='S-1-5-21-2865687314-2573020260-3100654925-1000'/></System><EventData><Data Name='FileNameLength'>77</Data><Data Name='File Name'>\Device\HarddiskVolume6\fry_storage\FryEdgeMiner\partners\titan\goworkerd.dll</Data><Data Name='ProcessNameLength'>78</Data><Data Name='Process Name'>\Device\HarddiskVolume6\fry_storage\FryEdgeMiner\partners\titan\titan-edge.exe</Data><Data Name='Requested Signing Level'>2</Data><Data Name='Validated Signing Level'>2</Data><Data Name='Status'>0xc0e90002</Data><Data Name='SHA1 Hash Size'>20</Data><Data Name='SHA1 Hash'>FDF21267BE3B2ABAD522540688B74837A11A0E73</Data><Data Name='SHA256 Hash Size'>32</Data><Data Name='SHA256 Hash'>FD6BBE11D86FB7B2FE19A901572A47A128FCB5DB7DCF034943A2A13D73B0DF94</Data><Data Name='SHA1 Flat Hash Size'>20</Data><Data Name='SHA1 Flat Hash'>01B96117C2409951BC48A067C565E6428E1A40AF</Data><Data Name='SHA256 Flat Hash Size'>32</Data><Data Name='SHA256 Flat Hash'>997CD9439ED79EA22C311DCCA7308604755517E15C2C3EE17CE96F41412609E6</Data><Data Name='USN'>0</Data><Data Name='SI Signing Scenario'>1</Data><Data Name='PolicyNameLength'>46</Data><Data Name='PolicyName'>FEMQA-B15-AUDIT lab policy (remove after cell)</Data><Data Name='PolicyIDLength'>6</Data><Data Name='PolicyID'>022422</Data><Data Name='PolicyHashSize'>32</Data><Data Name='PolicyHash'>F109BB9689C313B627782A488D85EA773F65BFC98F5B52E26F6E06E47A8EAFC4</Data><Data Name='OriginalFileNameLength'>0</Data><Data Name='OriginalFileName'></Data><Data Name='InternalNameLength'>0</Data><Data Name='InternalName'></Data><Data Name='FileDescriptionLength'>0</Data><Data Name='FileDescription'></Data><Data Name='ProductNameLength'>0</Data><Data Name='ProductName'></Data><Data Name='FileVersion'>0.0.0.0</Data><Data Name='PolicyGUID'>{d69c1cba-e1fc-489e-b177-a9ba36e1d7e6}</Data><Data Name='UserWriteable'>false</Data><Data Name='PackageFamilyNameLength'>0</Data><Data Name='PackageFamilyName'></Data></EventData></Event>"#;

/// EventRecordID 75 — same two source files, second event. Audit-mode
/// (3076): titan-edge.exe would have failed the same lab policy, but was
/// allowed to load.
const CAPTURED_TITAN_EDGE_AUDIT: &str = r#"<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='Microsoft-Windows-CodeIntegrity' Guid='{4ee76bd8-3cf4-44a0-a0ac-3937643e37a3}'/><EventID>3076</EventID><Version>5</Version><Level>4</Level><Task>18</Task><Opcode>118</Opcode><Keywords>0x8000000000000000</Keywords><TimeCreated SystemTime='2026-09-24T17:39:17.1330993Z'/><EventRecordID>75</EventRecordID><Correlation ActivityID='{a0081f4c-4bd7-0000-b9d1-0da0d74bdd01}'/><Execution ProcessID='2808' ThreadID='8112'/><Channel>Microsoft-Windows-CodeIntegrity/Operational</Channel><Computer>FEMQA-W11</Computer><Security UserID='S-1-5-21-2865687314-2573020260-3100654925-1000'/></System><EventData><Data Name='FileNameLength'>78</Data><Data Name='File Name'>\Device\HarddiskVolume6\fry_storage\FryEdgeMiner\partners\titan\titan-edge.exe</Data><Data Name='ProcessNameLength'>83</Data><Data Name='Process Name'>\Device\HarddiskVolume3\Users\femqa\AppData\Local\Fry Edge Miner\fry-edge-miner.exe</Data><Data Name='Requested Signing Level'>2</Data><Data Name='Validated Signing Level'>2</Data><Data Name='Status'>0xc0e90002</Data><Data Name='SHA1 Hash Size'>20</Data><Data Name='SHA1 Hash'>171AB5B521D37DF5D2E6E2986ED616489274EF26</Data><Data Name='SHA256 Hash Size'>32</Data><Data Name='SHA256 Hash'>E89C47AFFA5C36F8E9B81DB6567E424AEE49617CE9D02E08DC614A1296382C34</Data><Data Name='SHA1 Flat Hash Size'>20</Data><Data Name='SHA1 Flat Hash'>9A0828C82945B2BF2FF51E67CACF8952D36A3F59</Data><Data Name='SHA256 Flat Hash Size'>32</Data><Data Name='SHA256 Flat Hash'>A9E4A521343A1CE6800BA15178D7DA403CD48CCF15E3DF4ADC464969DCF95E8B</Data><Data Name='USN'>0</Data><Data Name='SI Signing Scenario'>1</Data><Data Name='PolicyNameLength'>46</Data><Data Name='PolicyName'>FEMQA-B15-AUDIT lab policy (remove after cell)</Data><Data Name='PolicyIDLength'>6</Data><Data Name='PolicyID'>022422</Data><Data Name='PolicyHashSize'>32</Data><Data Name='PolicyHash'>F109BB9689C313B627782A488D85EA773F65BFC98F5B52E26F6E06E47A8EAFC4</Data><Data Name='OriginalFileNameLength'>0</Data><Data Name='OriginalFileName'></Data><Data Name='InternalNameLength'>0</Data><Data Name='InternalName'></Data><Data Name='FileDescriptionLength'>0</Data><Data Name='FileDescription'></Data><Data Name='ProductNameLength'>0</Data><Data Name='ProductName'></Data><Data Name='FileVersion'>0.0.0.0</Data><Data Name='PolicyGUID'>{d69c1cba-e1fc-489e-b177-a9ba36e1d7e6}</Data><Data Name='UserWriteable'>false</Data><Data Name='PackageFamilyNameLength'>0</Data><Data Name='PackageFamilyName'></Data></EventData></Event>"#;

/// EventRecordID 73 — b15-fem-script-output.txt's third event (FEM's own
/// probe capture). Audit-mode (3076): fry-edge-miner.exe itself.
const CAPTURED_FEM_AUDIT: &str = r#"<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='Microsoft-Windows-CodeIntegrity' Guid='{4ee76bd8-3cf4-44a0-a0ac-3937643e37a3}'/><EventID>3076</EventID><Version>5</Version><Level>4</Level><Task>18</Task><Opcode>118</Opcode><Keywords>0x8000000000000000</Keywords><TimeCreated SystemTime='2026-09-24T17:39:02.8844218Z'/><EventRecordID>73</EventRecordID><Correlation ActivityID='{a0081f4c-4bd7-0000-a4ce-0da0d74bdd01}'/><Execution ProcessID='8620' ThreadID='9432'/><Channel>Microsoft-Windows-CodeIntegrity/Operational</Channel><Computer>FEMQA-W11</Computer><Security UserID='S-1-5-21-2865687314-2573020260-3100654925-1000'/></System><EventData><Data Name='FileNameLength'>83</Data><Data Name='File Name'>\Device\HarddiskVolume3\Users\femqa\AppData\Local\Fry Edge Miner\fry-edge-miner.exe</Data><Data Name='ProcessNameLength'>44</Data><Data Name='Process Name'>\Device\HarddiskVolume3\Windows\explorer.exe</Data><Data Name='Requested Signing Level'>2</Data><Data Name='Validated Signing Level'>2</Data><Data Name='Status'>0xc0e90002</Data><Data Name='SHA1 Hash Size'>20</Data><Data Name='SHA1 Hash'>51407D32C9D380846F51FF75D9460314488556B9</Data><Data Name='SHA256 Hash Size'>32</Data><Data Name='SHA256 Hash'>3D13E43137EFF9830B4098252445AA4216AA176E8C6CB3616F918805B7BC5523</Data><Data Name='SHA1 Flat Hash Size'>20</Data><Data Name='SHA1 Flat Hash'>810B9EE3D62BB94E223B4A5690DDA556246A4278</Data><Data Name='SHA256 Flat Hash Size'>32</Data><Data Name='SHA256 Flat Hash'>C982A3AD3DF844FACEB4A8527575C5291F376644E5F2AE367BD73B5B621CAACE</Data><Data Name='USN'>855427400</Data><Data Name='SI Signing Scenario'>1</Data><Data Name='PolicyNameLength'>46</Data><Data Name='PolicyName'>FEMQA-B15-AUDIT lab policy (remove after cell)</Data><Data Name='PolicyIDLength'>6</Data><Data Name='PolicyID'>022422</Data><Data Name='PolicyHashSize'>32</Data><Data Name='PolicyHash'>F109BB9689C313B627782A488D85EA773F65BFC98F5B52E26F6E06E47A8EAFC4</Data><Data Name='OriginalFileNameLength'>0</Data><Data Name='OriginalFileName'></Data><Data Name='InternalNameLength'>0</Data><Data Name='InternalName'></Data><Data Name='FileDescriptionLength'>14</Data><Data Name='FileDescription'>Fry Edge Miner</Data><Data Name='ProductNameLength'>14</Data><Data Name='ProductName'>Fry Edge Miner</Data><Data Name='FileVersion'>0.4.34.0</Data><Data Name='PolicyGUID'>{d69c1cba-e1fc-489e-b177-a9ba36e1d7e6}</Data><Data Name='UserWriteable'>false</Data><Data Name='PackageFamilyNameLength'>0</Data><Data Name='PackageFamilyName'></Data></EventData></Event>"#;

/// The exact separator PowerShell's `ForEach-Object { $_.ToXml(); '' }`
/// produces between events, and after the trailing one — reproduced from
/// b15-fem-script-output.txt (`od -c`: `</Event>` then CR LF CR LF).
fn crlf_listing(events: &[&str]) -> String {
    let mut out = String::new();
    for event in events {
        out.push_str(event);
        out.push_str("\r\n\r\n");
    }
    out
}

/// A `now` inside the captures' own window (they span 17:39:02–17:39:20).
fn captured_now() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-09-24T17:39:25Z")
        .expect("fixed clock")
        .with_timezone(&chrono::Utc)
}

// ---------------------------------------------------------------------
// FAIL-9: an audit-mode (3076) event must not suppress recovery.
// ---------------------------------------------------------------------

/// Fixture sanity: the negative result below is a BLOCK-vs-AUDIT decision,
/// not a name-matching failure — the captured event really does mention
/// titan-edge.exe, under one of the ids the probe queries for.
#[test]
fn the_captured_audit_event_names_our_image() {
    assert!(event_names_our_image(
        CAPTURED_TITAN_EDGE_AUDIT,
        &titan_edge()
    ));
}

#[test]
fn a_real_captured_audit_only_event_is_not_treated_as_a_block() {
    let listing = crlf_listing(&[CAPTURED_TITAN_EDGE_AUDIT]);
    assert!(
        first_block_for_at(&listing, &titan_edge(), captured_now(), BLOCK_RECENCY).is_none(),
        "EventID 3076 under an AUDIT-mode policy means titan-edge.exe WAS \
         allowed to load — treating it as a block would show 'Awaiting \
         administrator action' for a process that is running"
    );
}

/// The fence's own acceptance test: "A 3077 variant of the same XML (event id
/// changed) MUST [produce a block]." Non-vacuity for the test above — proves
/// the miss is about audit-vs-enforced, not about the matcher going blind.
#[test]
fn a_3077_variant_of_the_same_captured_event_is_a_block() {
    let enforced = CAPTURED_TITAN_EDGE_AUDIT.replacen("3076", "3077", 1);
    let listing = crlf_listing(&[enforced.as_str()]);
    let found = first_block_for_at(&listing, &titan_edge(), captured_now(), BLOCK_RECENCY)
        .expect("an enforced refusal (3077) for the same image must still be caught");
    assert!(found.contains("titan-edge.exe"));
}

// ---------------------------------------------------------------------
// FAIL-10: a CRLF listing must split per event, each judged by its own
// SystemTime — not the first timestamp in the raw string.
// ---------------------------------------------------------------------

/// Real-data proof of chunk isolation: the block returned for titan-edge.exe
/// must be titan-edge.exe's OWN event (EventRecordID 75) — not the whole
/// three-event CRLF blob, which is what an unsplit listing hands back.
/// EventRecordID is the signal (not file names): each of these events'
/// "Process Name" field legitimately names ANOTHER file in this same
/// capture, so a file-name-based check can't tell isolation from merging.
#[test]
fn the_returned_block_is_the_single_matching_event_not_the_whole_captured_listing() {
    let enforced_titan_edge = CAPTURED_TITAN_EDGE_AUDIT.replacen("3076", "3077", 1);
    let listing = crlf_listing(&[
        CAPTURED_GOWORKERD_AUDIT,
        &enforced_titan_edge,
        CAPTURED_FEM_AUDIT,
    ]);

    let found = first_block_for_at(&listing, &titan_edge(), captured_now(), BLOCK_RECENCY)
        .expect("the enforced block for titan-edge.exe must be found");
    assert!(
        found.contains("<EventRecordID>75</EventRecordID>"),
        "must be titan-edge.exe's own event: {found}"
    );
    assert!(
        !found.contains("<EventRecordID>77</EventRecordID>")
            && !found.contains("<EventRecordID>73</EventRecordID>"),
        "the real CRLF capture must split into per-event chunks — a chunk \
         that still contains the OTHER two events' EventRecordIDs means the \
         listing was never split and this is the whole blob, not one event: \
         {found}"
    );
}

/// The decisive semantic proof: a fresh, enforced block for OUR image must be
/// found even though an OLDER, unrelated event precedes it in the same CRLF
/// listing — exactly the shape `Get-WinEvent` returns (most recent first).
/// Real captured timestamps span only 18 seconds, too close together to ever
/// show a wrong RECENCY VERDICT (only wrong chunk CONTENT, proven above), so
/// this drives the same real CRLF-separator shape with timestamps far enough
/// apart that judging the wrong chunk's SystemTime flips Some/None.
#[test]
fn a_recent_block_is_found_even_behind_an_older_unrelated_event_in_the_same_crlf_listing() {
    fn event_at(file_name: &str, id: &str, at: chrono::DateTime<chrono::Utc>) -> String {
        format!(
            "<Event><System><EventID>{id}</EventID>\
             <TimeCreated SystemTime='{}'/></System>\
             <EventData><Data Name='File Name'>\\Device\\HarddiskVolume6\\{file_name}</Data></EventData></Event>",
            at.to_rfc3339()
        )
    }

    let now = chrono::Utc::now();
    let old_unrelated = event_at(
        "unrelated-tool.exe",
        "3077",
        now - chrono::Duration::hours(2),
    );
    let fresh_block = event_at("goworkerd.dll", "3077", now - chrono::Duration::seconds(5));
    let listing = crlf_listing(&[&old_unrelated, &fresh_block]);

    let found = first_block_for_at(&listing, &goworkerd(), now, BLOCK_RECENCY).expect(
        "a fresh, enforced block naming our image must be found regardless of \
         an older unrelated event earlier in the same CRLF listing",
    );
    assert!(found.contains("goworkerd.dll"));
}

// ---------------------------------------------------------------------
// BUG LOOP 2 (NB): the enforced-vs-audit check used to match an id
// ANYWHERE in the event XML (`>{id}<` or `'{id}'`), not specifically inside
// the `<EventID>` element — so a genuinely audit-only 3076 event whose
// EventRecordID, USN, ProcessID or ThreadID happened to equal 3033/3077 was
// misread as an ENFORCED block. Only the `<EventID>` element may decide.
// ---------------------------------------------------------------------

/// The exact real capture, with ONLY its EventRecordID changed to 3077 —
/// `<EventID>` stays 3076 (audit). Everything else (File Name, PolicyName,
/// the real hashes) is untouched real-capture data.
#[test]
fn a_3076_audit_event_whose_eventrecordid_is_3077_is_still_not_a_block() {
    let decoy = CAPTURED_TITAN_EDGE_AUDIT.replacen(
        "<EventRecordID>75</EventRecordID>",
        "<EventRecordID>3077</EventRecordID>",
        1,
    );
    assert!(
        decoy.contains("<EventID>3076</EventID>"),
        "fixture sanity: EventID must stay 3076: {decoy}"
    );
    assert!(
        decoy.contains("<EventRecordID>3077</EventRecordID>"),
        "fixture sanity: EventRecordID must now read 3077: {decoy}"
    );

    assert!(
        event_names_our_image(&decoy, &titan_edge()),
        "fixture sanity: the decoy must still name titan-edge.exe"
    );

    let listing = crlf_listing(&[decoy.as_str()]);
    assert!(
        first_block_for_at(&listing, &titan_edge(), captured_now(), BLOCK_RECENCY).is_none(),
        "a 3076 (audit) event must not become an enforced block just because \
         its EventRecordID happens to equal 3077 — only <EventID> may decide: {decoy}"
    );
}

/// The same decoy shape, on the `USN` Data value instead of EventRecordID —
/// the other field lens 1 flagged as an equally reachable false-positive
/// path through the old id-anywhere matcher.
#[test]
fn a_3076_audit_event_whose_usn_is_3033_is_still_not_a_block() {
    let decoy = CAPTURED_TITAN_EDGE_AUDIT.replacen(
        "<Data Name='USN'>0</Data>",
        "<Data Name='USN'>3033</Data>",
        1,
    );
    assert!(
        decoy.contains("<Data Name='USN'>3033</Data>"),
        "fixture sanity: USN must now read 3033: {decoy}"
    );
    assert!(
        decoy.contains("<EventID>3076</EventID>"),
        "fixture sanity: {decoy}"
    );

    let listing = crlf_listing(&[decoy.as_str()]);
    assert!(
        first_block_for_at(&listing, &titan_edge(), captured_now(), BLOCK_RECENCY).is_none(),
        "a 3076 (audit) event must not become an enforced block just because \
         an unrelated Data value happens to equal 3033: {decoy}"
    );
}
