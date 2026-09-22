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

/// PURE: the first block in `listing` that names `image`, if any. The listing
/// is one event's XML per chunk, separated by blank lines.
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
    format!(
        "Get-WinEvent -FilterHashtable @{{LogName='Microsoft-Windows-CodeIntegrity/Operational'; \
         Id={}}} -MaxEvents 20 -ErrorAction SilentlyContinue | \
         ForEach-Object {{ $_.ToXml(); '' }}",
        BLOCK_EVENT_IDS.join(",")
    )
}

/// Ask the event log whether Windows recently refused to load `image`, and if
/// so what to tell the user. `None` means "no evidence of a block" — never
/// "everything is fine", so the caller keeps its own diagnosis.
pub(crate) fn recent_block(image: &Path) -> Option<String> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    let out = crate::supervisor::platform::command("powershell")
        .args(["-NoProfile", "-Command", &recent_blocks_script()])
        .output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)
        .ok()?;
    let listing = String::from_utf8_lossy(&out.stdout);
    first_block_for(&listing, image).map(|_| user_message(image))
}

#[cfg(test)]
#[path = "code_integrity_tests.rs"]
mod code_integrity_tests;
