//! B15 defect 5: an OS code-integrity block is not a crash, and restarting
//! through it forever is not a recovery.
//!
//! Windows' Smart App Control and WDAC refuse to load an unsigned image. The
//! child dies immediately, its logs are truncated on every respawn
//! (`ManagedProcess::spawn_full` re-creates both files), so titan's health
//! check finds no "Daemon"/"edge"/"listening" marker and returns `Starting` —
//! which `recovery_action` turns into `StartupTimedOut` after ~3 minutes and
//! then re-arms one attempt every ~5 minutes, forever. Nothing in FEM ever
//! reads Microsoft-Windows-CodeIntegrity/Operational, and the only escape from
//! `RecoveryAction::Restart` is `awaits_user_action`.
//!
//! So: recognise the block, say what it is in words the user can act on, and
//! start the reason with a marker `awaits_user_action` knows — which routes the
//! health loop to `RecoveryAction::None` and stops the respawn.
//!
//! The block is the OS refusing to load the image, NOT a corrupted download: a
//! pinned sha256 PASSES on a byte-intact DLL that SAC blocks. The two causes
//! are told apart by which check fires, not by the symptom.

use std::path::Path;

use crate::supervisor::platform::BoundedOutput;

/// The prefix `integrations::awaits_user_action` matches on. Changing it
/// without changing that list re-arms the respawn loop.
pub(crate) const AWAITING_ADMIN_MARKER: &str = "Awaiting administrator action";

/// The CodeIntegrity event ids that mean "this image was refused".
/// 3033: the file did not meet the signing-level requirement. 3077: the file
/// would have been blocked (audit). 3076: an audit-mode block.
const BLOCK_EVENT_IDS: [&str; 3] = ["3033", "3076", "3077"];

/// What the card shows. Starts with the marker so the health loop stops
/// restarting, and then says the three things the user needs: which file, what
/// did it, and that this is not a broken download.
pub(crate) fn user_message(image: &Path) -> String {
    let name = image_file_name(image).unwrap_or_else(|| image.to_string_lossy().into_owned());
    format!(
        "{AWAITING_ADMIN_MARKER} — Windows blocked {name} from loading (Smart App Control or an \
         app-control policy refused the unsigned file). The file itself is intact, so \
         reinstalling will not help. Allow {name} in Windows Security, or turn Smart App Control \
         off, then re-enable this integration."
    )
}

/// The last path component of `image`, splitting on EITHER separator.
///
/// `Path::file_name` is host-dependent: on a non-Windows host it does not treat
/// `\` as a separator, so a Windows path comes back as one whole component.
/// This module is about Windows paths by definition — the event log reports NT
/// device paths and FEM holds drive-letter paths — so the split has to be
/// explicit rather than inherited from whatever host the code is compiled on.
fn image_file_name(image: &Path) -> Option<String> {
    let text = image.to_string_lossy();
    let name = text.rsplit(['\\', '/']).next()?;
    (!name.is_empty()).then(|| name.to_string())
}

/// PURE: does this event XML name the image we are asking about, and is it one
/// of the block ids?
///
/// Matched on the file NAME, not the full path: the event log reports an NT
/// device path (`\Device\HarddiskVolume3\Users\...`) rather than the drive
/// letter FEM knows the file by, so a full-path comparison would never match.
pub(crate) fn event_names_our_image(event_xml: &str, image: &Path) -> bool {
    let Some(name) = image_file_name(image) else {
        return false;
    };
    let name = name.to_lowercase();
    let haystack = event_xml.to_lowercase();
    if !haystack.contains(&name) {
        return false;
    }
    BLOCK_EVENT_IDS
        .iter()
        .any(|id| haystack.contains(&format!(">{id}<")) || haystack.contains(&format!("'{id}'")))
}

/// How recent a block event has to be to describe the CURRENT state.
///
/// G4 findings 6 and 16: nothing here filtered by time, and the
/// CodeIntegrity/Operational channel is near-silent on a normal machine — so
/// ONE historical block survived in the last 20 events indefinitely. A user who
/// followed FEM's own instruction, allowed the file and ran for weeks would, on
/// the next ordinary death (disk full, upstream crash, ended task), have that
/// months-old event re-read, the "Awaiting administrator action" marker
/// returned, and `recovery_action` suppressed FOREVER. A one-off historical
/// event permanently converted a restartable failure into an un-restartable one
/// with instructions for a problem the user had already fixed.
///
/// The window can be short because a genuinely blocked image writes a NEW event
/// on every load attempt: if the block is real, the next spawn re-proves it. And
/// because suppression stops the spawns, the window expiring is what lets FEM
/// try again — a fixed machine self-heals instead of needing a toggle.
pub(crate) const BLOCK_RECENCY: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// PURE: the `SystemTime` attribute of an event's `<TimeCreated>` element.
///
/// `None` when absent or unparseable, which the caller treats as NOT recent —
/// failing CLOSED here, unlike titan's log-line recency which fails open. The
/// asymmetry is deliberate: an unreadable timestamp on a log line should still
/// surface a diagnosis, whereas an unreadable timestamp here would suppress
/// recovery, and suppression is the dangerous direction.
pub(crate) fn event_time(event_xml: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let at = event_xml.find("SystemTime=")?;
    let rest = &event_xml[at + "SystemTime=".len()..];
    let quote = rest.chars().next()?;
    let value = rest[1..].split(quote).next()?;
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|t| t.with_timezone(&chrono::Utc))
}

/// PURE: is this event recent enough to describe the current state?
pub(crate) fn event_is_recent(
    event_xml: &str,
    now: chrono::DateTime<chrono::Utc>,
    window: std::time::Duration,
) -> bool {
    let Some(stamped) = event_time(event_xml) else {
        return false;
    };
    let age = now.signed_duration_since(stamped);
    age >= chrono::Duration::zero() && age <= chrono::Duration::from_std(window).unwrap_or_default()
}

/// PURE: the first RECENT block in `listing` that names `image`, if any. The
/// listing is one event's XML per chunk, separated by blank lines.
pub(crate) fn first_block_for_at(
    listing: &str,
    image: &Path,
    now: chrono::DateTime<chrono::Utc>,
    window: std::time::Duration,
) -> Option<String> {
    listing
        .split("\n\n")
        .map(str::trim)
        .find(|chunk| {
            !chunk.is_empty()
                && event_names_our_image(chunk, image)
                && event_is_recent(chunk, now, window)
        })
        .map(|chunk| chunk.to_string())
}

/// The name-only form the existing tests pin. Kept so those tests stay
/// byte-identical; production goes through `first_block_for_at`, which also
/// requires the event to be recent.
#[allow(dead_code)]
pub(crate) fn first_block_for(listing: &str, image: &Path) -> Option<String> {
    listing
        .split("\n\n")
        .map(str::trim)
        .find(|chunk| !chunk.is_empty() && event_names_our_image(chunk, image))
        .map(|chunk| chunk.to_string())
}

/// PURE: the PowerShell that dumps the recent CodeIntegrity block events.
/// `-ErrorAction SilentlyContinue` because the channel is disabled on some
/// installs, which is not an error condition for FEM.
pub(crate) fn recent_blocks_script() -> String {
    // StartTime bounds the query at the source, so a near-silent channel cannot
    // hand back a months-old event. The Rust side filters again on the parsed
    // TimeCreated, because this string is only as good as the host's clock and
    // the Rust check is the one that is unit-testable.
    format!(
        "Get-WinEvent -FilterHashtable @{{LogName='Microsoft-Windows-CodeIntegrity/Operational'; \
         Id={}; StartTime=(Get-Date).AddSeconds(-{})}} -MaxEvents 20 -ErrorAction SilentlyContinue | \
         ForEach-Object {{ $_.ToXml(); '' }}",
        BLOCK_EVENT_IDS.join(","),
        BLOCK_RECENCY.as_secs()
    )
}

/// Ask the event log whether Windows recently refused to load `image`, and if
/// so what to tell the user. `None` means "no evidence of a block" — never
/// "everything is fine", so the caller keeps its own diagnosis.
pub(crate) fn recent_block(image: &Path) -> Option<String> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    // Findings 6/16 secondary cost: this is consulted on EVERY health tick while
    // the process is down, so without a memo it spawns one or two 20 s-bounded
    // PowerShell probes every 30 s indefinitely. A block does not appear and
    // disappear within seconds, so a short memo costs nothing in accuracy.
    if let Some(cached) = cached_listing() {
        return first_block_for_at(&cached, image, chrono::Utc::now(), BLOCK_RECENCY)
            .map(|_| user_message(image));
    }
    let out = crate::supervisor::platform::command("powershell")
        .args(["-NoProfile", "-Command", &recent_blocks_script()])
        .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
        .ok()?;
    let listing = String::from_utf8_lossy(&out.stdout).to_string();
    remember_listing(&listing);
    first_block_for_at(&listing, image, chrono::Utc::now(), BLOCK_RECENCY)
        .map(|_| user_message(image))
}

/// How long a listing is reused before the channel is queried again.
const LISTING_MEMO: std::time::Duration = std::time::Duration::from_secs(60);

static LISTING_CACHE: std::sync::Mutex<Option<(String, std::time::Instant)>> =
    std::sync::Mutex::new(None);

fn cached_listing() -> Option<String> {
    let guard = LISTING_CACHE.lock().ok()?;
    let (listing, at) = guard.as_ref()?;
    (at.elapsed() < LISTING_MEMO).then(|| listing.clone())
}

fn remember_listing(listing: &str) {
    if let Ok(mut guard) = LISTING_CACHE.lock() {
        *guard = Some((listing.to_string(), std::time::Instant::now()));
    }
}

#[cfg(test)]
#[path = "code_integrity_tests.rs"]
mod code_integrity_tests;

/// G4 findings 6 and 16: the recency filter that stops a historical block
/// permanently disabling recovery. Separate file so `code_integrity_tests.rs`
/// stays byte-identical.
#[cfg(test)]
#[path = "code_integrity_recency_tests.rs"]
mod code_integrity_recency_tests;
