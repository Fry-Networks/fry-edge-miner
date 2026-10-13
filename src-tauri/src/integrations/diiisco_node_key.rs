//! Per-device Diiisco node API key.
//!
//! The bearer is the local node's own client API key: the Docker build bakes it
//! into this device's node and the health probe presents it back. When neither
//! the env override nor the backend credential supplies one, the device mints
//! its own random key once and keeps it in the deploy dir. It never leaves the
//! device and is never logged.
use crate::api::types::CredentialInfo;
use std::path::Path;

#[cfg_attr(not(test), allow(dead_code))]
pub(super) const KEY_FILE: &str = "node-api-key";
#[cfg_attr(not(test), allow(dead_code))]
pub(super) const KEY_PREFIX: &str = "sk-fem-";

#[cfg_attr(not(test), allow(dead_code))]
static KEY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Exactly `KEY_PREFIX` followed by 64 lowercase hex characters.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn is_valid_key(_s: &str) -> bool {
    false
}

/// Return the persisted key, minting and persisting one if absent or invalid.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn load_or_create_key(_dir: &Path) -> anyhow::Result<String> {
    Ok(String::new())
}

/// The persisted key if it is valid; never creates one.
pub(super) fn read_key(_dir: &Path) -> Option<String> {
    None
}

/// Fill the credential's bearer from the device key when neither the env
/// override nor the served field provides one.
pub(super) fn fill_local_key(
    creds: &mut CredentialInfo,
    env: Option<String>,
    dir: &Path,
) -> anyhow::Result<()> {
    let _ = (creds, env, dir);
    Ok(())
}

#[cfg(test)]
#[path = "diiisco_node_key_tests.rs"]
mod tests;
