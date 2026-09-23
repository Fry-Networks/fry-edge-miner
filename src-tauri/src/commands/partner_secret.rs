//! B17 D6: the backend half of in-app entry for a single-value partner secret.
//!
//! The Done-when is specific about three things, and all three are the reason
//! this is a command rather than a settings field: the value is masked in the
//! UI, it is written WHERE THE INTEGRATION ALREADY READS IT, and it is excluded
//! from logs and bundles.
//!
//! "Where the integration already reads it" is the load-bearing clause. Iagon's
//! own reader is `token_from_config` -> `<partners>/iagon/config.json` ->
//! `{"node_token": "..."}`, and it re-reads on every call precisely so a user
//! can paste a key and toggle without restarting. Writing anywhere else — a new
//! FemConfig field, a keyring, a sidecar file — would mean the value is stored
//! and still not used, which is worse than not storing it.
//!
//! Exclusion from logs and bundles is NOT something this module does by adding
//! a rule. It is a consequence of never putting the value in a tracing event:
//! nothing here logs `value`, and the only place it lands is the JSON file the
//! integration reads. The bundle collector scrubs what it collects, and
//! `redact_json_secret` covers `"node_token": "..."` if that file is ever
//! collected.

use std::path::PathBuf;

/// The one-token integrations, and the file plus key each already reads.
///
/// An explicit allow-list, not a free-form path: the id arrives from the
/// frontend, and a command that writes an arbitrary key into an arbitrary file
/// under the partners root would be a much larger surface than the feature
/// needs. An unknown id fails closed.
fn secret_target(id: &str) -> Option<(PathBuf, &'static str)> {
    match id {
        "iagon" => Some((
            crate::integrations::download::partners_base_dir()
                .join("iagon")
                .join("config.json"),
            "node_token",
        )),
        _ => None,
    }
}

/// Merge `key = value` into the JSON object in `existing`, preserving every
/// other key.
///
/// Pure, so the merge is testable without a disk: a user who has hand-edited
/// that config.json — which the Iagon card's own guidance tells them to do —
/// must not lose whatever else they put in it. A file that is absent, empty, or
/// not a JSON object is replaced with a fresh object rather than refused, since
/// the alternative is telling the user their own broken file blocks them.
pub(crate) fn merge_secret(existing: Option<&str>, key: &str, value: &str) -> String {
    // Same BOM tolerance as Iagon's reader: PowerShell's `Set-Content
    // -Encoding utf8` and older Notepad prepend one, and serde rejects it.
    let parsed = existing
        .map(|raw| raw.trim_start_matches('\u{feff}'))
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok());
    let mut object = match parsed {
        Some(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    object.insert(
        key.to_string(),
        serde_json::Value::String(value.to_string()),
    );
    serde_json::Value::Object(object).to_string()
}

/// Save a single-value secret for `id`, in the place that integration reads it.
///
/// Deliberately returns only a message on error and never echoes `value`: an
/// error string reaches the card, `last_integration_error` and the log, and a
/// token that travelled that far would defeat the point of the masked field.
#[tauri::command]
pub async fn set_partner_secret(id: String, value: String) -> Result<(), String> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return Err("Enter the key before saving.".to_string());
    }

    let (path, key) =
        secret_target(&id).ok_or_else(|| format!("'{id}' does not take a single-value key."))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create {}: {e}", parent.display()))?;
    }
    let existing = std::fs::read_to_string(&path).ok();
    let merged = merge_secret(existing.as_deref(), key, &trimmed);

    // Same temp + rename discipline as the config store, so a crash mid-write
    // cannot leave the integration reading a truncated file. The temp name
    // carries the pid for the same reason it does there.
    let tmp = path.with_file_name(format!(
        "{}.tmp.{}",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "config.json".to_string()),
        std::process::id()
    ));
    std::fs::write(&tmp, merged.as_bytes()).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Could not write {}: {e}", path.display())
    })?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Could not save to {}: {e}", path.display())
    })?;

    // The KEY NAME and the PATH are safe to log; the value is not, and is not
    // in scope here by construction.
    tracing::info!(
        integration = %id,
        key,
        path = %path.display(),
        "Saved a partner key entered in the app"
    );
    Ok(())
}

#[cfg(test)]
#[path = "partner_secret_tests.rs"]
mod partner_secret_tests;
