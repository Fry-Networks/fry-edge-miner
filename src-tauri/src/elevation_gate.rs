//! B3: the single gate every elevation request in FEM passes through.
//!
//! Before this module, all five elevation sites (`firewall::ensure_program_rules`,
//! `firewall::delete_rules`, `security_setup::run_hardening_elevated`,
//! `titan::install_vc_redist_elevated`, `docker_manager::run_docker_installer`)
//! raised `Start-Process -Verb RunAs` straight from whatever called them —
//! including the boot pass, the per-integration health loop and the Docker
//! watcher. A declined prompt left no record anywhere, so the next tick asked
//! again: the reported "endless PowerShell admin right request" with dozens of
//! stacked UAC entries in the taskbar.
//!
//! The gate enforces exactly what B3's Done-when asks for:
//!
//! * `Automatic` triggers NEVER elevate. FEM raises no UAC prompt the user did
//!   not ask for, full stop. The caller gets `ElevationSkipped::NeedsApproval`
//!   and the reason is published for the UI.
//! * A `UserClick` elevates at most once per `attempt_key` per process run, so
//!   a declined or ignored prompt is not re-raised behind the user's back. The
//!   key is identity-bearing (rule name + program path, or the version being
//!   hardened), so a genuinely NEW target — OlostepBrowser's post-Squirrel
//!   path, the updater's new version — is a new key and still gets its one
//!   attempt.
//! * Elevations are serialised, never concurrent: the gate mutex is held
//!   ACROSS the caller's closure. It serialises rather than suppresses, so a
//!   second request waits instead of being silently dropped.
//!
//! Every call site already treats its elevation as best-effort and has a
//! `warn!(… "continuing")` branch on `Err` (firewall.rs:93-95, fryvpn.rs:325,
//! aem.rs:377, security_setup.rs:124-127), so `ElevationSkipped` takes a path
//! those sites already exercise.
//!
//! ## Public API — FROZEN
//!
//! Call sites outside this module depend on these items and their exact
//! signatures: `ElevationTrigger`, `ElevationSkipped`, `NEEDS_APPROVAL_MESSAGE`,
//! `run_elevated`, `is_declined`, `blocked_reasons`, `clear_blocked`.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use tracing::{info, warn};

/// The exact string B3's Done-when requires the card to show when an elevation
/// is suppressed or declined. Delivered through the existing `IntCard` error
/// slot (`commands::integration::get_integrations` -> `IntCard.tsx`), whose
/// condenser passes a single-line message through unchanged.
pub const NEEDS_APPROVAL_MESSAGE: &str = "Needs administrator approval — Retry";

/// What caused this elevation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationTrigger {
    /// A human clicked something in the UI that asks for exactly this action.
    UserClick,
    /// A health tick, the boot pass, the Docker watcher, an update step —
    /// anything the user did not ask for at this moment.
    Automatic,
}

/// Why `run_elevated` did not hand back the caller's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElevationSkipped {
    /// The trigger was `Automatic`. FEM does not raise UAC on its own; the
    /// user has to ask for it.
    NeedsApproval,
    /// This exact `attempt_key` was already attempted in this process run, so
    /// the prompt is not raised a second time behind the user's back.
    AlreadyAttempted,
    /// The work ran and failed — a declined prompt, or a genuine error. The
    /// string is what the card shows.
    Failed(String),
}

impl std::fmt::Display for ElevationSkipped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ElevationSkipped::NeedsApproval => f.write_str(NEEDS_APPROVAL_MESSAGE),
            ElevationSkipped::AlreadyAttempted => {
                f.write_str("already asked for administrator approval once this session")
            }
            ElevationSkipped::Failed(reason) => f.write_str(reason),
        }
    }
}

#[derive(Default)]
struct GateState {
    /// Every `attempt_key` a `UserClick` has already spent its one shot on.
    attempted: HashSet<String>,
}

/// Serialises the elevations themselves. Held ACROSS the caller's closure, so
/// it can be held for as long as a human takes to answer a UAC prompt — never
/// read it from a UI path.
fn gate() -> &'static Mutex<GateState> {
    static GATE: OnceLock<Mutex<GateState>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(GateState::default()))
}

/// The reasons the UI reads. A SEPARATE lock from `gate()` on purpose: a poll
/// of `get_integrations` must never block behind an on-screen UAC prompt.
fn blocked_map() -> &'static Mutex<HashMap<String, String>> {
    static BLOCKED: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    BLOCKED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Pure: does this raw outcome mean the human did not approve, rather than the
/// elevated work itself going wrong?
///
/// Exit 2 and 3 are the outer wrapper's own "UAC declined/cancelled" codes (see
/// `security_setup::hardening_outcome`), 1223 is Windows' `ERROR_CANCELLED`,
/// and a `TimedOut` io error means `output_bounded` gave up because nobody
/// answered the prompt inside the budget.
pub fn is_declined(exit_code: Option<i32>, io_kind: Option<std::io::ErrorKind>) -> bool {
    matches!(exit_code, Some(2) | Some(3) | Some(1223))
        || io_kind == Some(std::io::ErrorKind::TimedOut)
}

/// The io error kind anywhere in an `anyhow` chain, if there is one.
fn io_kind_of(err: &anyhow::Error) -> Option<std::io::ErrorKind> {
    err.chain()
        .find_map(|c| c.downcast_ref::<std::io::Error>())
        .map(|io| io.kind())
}

/// Run `f` — which is expected to raise a UAC prompt — under the gate.
///
/// `purpose` is the integration id when the elevation belongs to one
/// integration (`"fryvpn"`, `"aem"`, `"titan"`), so `blocked_reason` can be
/// merged straight into that card's error slot; otherwise it is a stable name
/// that is not an integration id (`"hardening"`, `"docker-desktop"`).
///
/// `attempt_key` identifies the exact target, so a changed target re-arms the
/// one allowed attempt.
pub fn run_elevated<T>(
    purpose: &'static str,
    attempt_key: &str,
    trigger: ElevationTrigger,
    f: impl FnOnce() -> anyhow::Result<T>,
) -> Result<T, ElevationSkipped> {
    if trigger == ElevationTrigger::Automatic {
        info!(purpose, trigger = "automatic", "elevation suppressed");
        publish_block(purpose, NEEDS_APPROVAL_MESSAGE.to_string());
        return Err(ElevationSkipped::NeedsApproval);
    }

    // Held across `f` on purpose: two UAC prompts must never be on screen at
    // once, and a second request must WAIT rather than be dropped.
    let mut state = gate().lock().unwrap_or_else(|e| e.into_inner());
    if !state.attempted.insert(attempt_key.to_string()) {
        info!(
            purpose,
            trigger = "user_click",
            "elevation suppressed — this exact request already had its one attempt this run"
        );
        return Err(ElevationSkipped::AlreadyAttempted);
    }
    info!(purpose, trigger = "user_click", "elevation allowed");
    let outcome = f();
    drop(state);

    match outcome {
        Ok(value) => {
            clear_blocked(purpose);
            Ok(value)
        }
        Err(e) => {
            let declined = is_declined(None, io_kind_of(&e));
            let reason = if declined {
                NEEDS_APPROVAL_MESSAGE.to_string()
            } else {
                e.to_string()
            };
            warn!(purpose, declined, error = %e, "elevation attempt failed");
            publish_block(purpose, reason.clone());
            Err(ElevationSkipped::Failed(reason))
        }
    }
}

fn publish_block(purpose: &str, reason: String) {
    blocked_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(purpose.to_string(), reason);
}

/// Snapshot of every currently-blocked purpose, keyed as `run_elevated`'s
/// `purpose` — for integration-scoped elevations that is the integration id,
/// so `commands::integration::get_integrations` can merge it straight into
/// that card's existing `error` slot.
pub fn blocked_reasons() -> HashMap<String, String> {
    blocked_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Forget a block. Called on a successful elevation, and by the retry gesture
/// (toggling the integration off and on) before it asks again.
pub fn clear_blocked(purpose: &str) {
    blocked_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(purpose);
}

#[cfg(test)]
#[path = "elevation_gate_tests.rs"]
mod elevation_gate_tests;
