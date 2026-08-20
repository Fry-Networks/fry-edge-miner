//! Consent commands for the Pawns.app integration.
//!
//! The Pawns.app CLI Addendum (§5.2–5.4) requires an explicit consent action
//! from the device owner before bandwidth sharing starts, and (§5.8) a durable
//! record of every consent and withdrawal. Consent used to be readable only
//! from `PAWNS_USER_CONSENT`, which the UI cannot set and the machine forgets —
//! so there was no way for an owner to actually give it. These commands are
//! that path: the log under `%APPDATA%/FryEdgeMiner/partners/pawns` is the
//! durable record, and the UI reads the audited wording from here rather than
//! keeping its own copy that could drift out of step with what gets recorded.

use crate::integrations::pawns::{self, PawnsIntegration};

/// The only integration with a consent requirement today.
const CONSENT_INTEGRATION_ID: &str = "pawns";

/// What the UI needs to decide whether to prompt, and what to show if it does.
#[derive(serde::Serialize)]
pub struct ConsentStatus {
    pub integration_id: String,
    /// Whether this device holds a recorded consent it has not withdrawn.
    pub active: bool,
    pub wording_version: String,
    /// The audited disclosure text, so the UI never retypes it.
    pub disclosure: String,
    /// The terms document the owner accepts, for the dialog to link.
    pub terms_url: String,
    pub terms_version: String,
    /// When the deciding consent/withdrawal was recorded, or None if never.
    pub recorded_at: Option<String>,
}

fn ensure_consent_integration(integration_id: &str) -> Result<(), String> {
    if integration_id != CONSENT_INTEGRATION_ID {
        return Err("Consent is only tracked for the Pawns.app integration".to_string());
    }
    Ok(())
}

/// Current consent state plus the wording to show if consent is missing.
#[tauri::command]
pub async fn check_consent(integration_id: String) -> Result<ConsentStatus, String> {
    ensure_consent_integration(&integration_id)?;

    let record = PawnsIntegration::consent_record();
    let active = record.as_ref().map(|r| r.action == "consent").unwrap_or(false);
    let recorded_at = record
        .map(|r| r.happened_at)
        .filter(|t| !t.is_empty());

    Ok(ConsentStatus {
        integration_id,
        active,
        wording_version: pawns::consent_wording_version().to_string(),
        disclosure: pawns::consent_disclosure().to_string(),
        terms_url: pawns::terms_url().to_string(),
        terms_version: pawns::terms_version().to_string(),
        recorded_at,
    })
}

/// Record the device owner agreeing to the disclosure they were shown.
///
/// `wording_version` is the version the dialog actually rendered. A dialog left
/// open across an update would otherwise record consent to wording the owner
/// never read, so a mismatch is refused rather than silently accepted.
#[tauri::command]
pub async fn grant_consent(integration_id: String, wording_version: String) -> Result<(), String> {
    ensure_consent_integration(&integration_id)?;

    let current = pawns::consent_wording_version();
    if wording_version != current {
        return Err(format!(
            "This consent notice is out of date (you were shown version {}, the current notice is \
             version {}). Close and reopen the dialog to read the current notice.",
            wording_version, current
        ));
    }

    PawnsIntegration::grant_consent();
    tracing::info!(integration = integration_id, "Pawns.app consent recorded");
    Ok(())
}

/// Record the device owner withdrawing consent.
///
/// The normal disable path records its own withdrawal through the integration's
/// `stop()`; this command covers an explicit withdrawal, and the UI's fallback
/// when a toggle failed before `stop()` could run.
#[tauri::command]
pub async fn revoke_consent(integration_id: String) -> Result<(), String> {
    ensure_consent_integration(&integration_id)?;

    PawnsIntegration::revoke_consent();
    tracing::info!(integration = integration_id, "Pawns.app consent withdrawn");
    Ok(())
}
