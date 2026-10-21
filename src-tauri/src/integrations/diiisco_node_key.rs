//! Per-device Diiisco node API key.
//!
//! The bearer is the local node's own client API key: the Docker build bakes it
//! into this device's node and the health probe presents it back. When neither
//! the env override nor the backend credential supplies one, the device mints
//! its own random key once and keeps it in the deploy dir. It never leaves the
//! device and is never logged.
use crate::api::types::CredentialInfo;
use rand::RngCore;
use std::path::Path;
use tracing::{info, warn};

pub(super) const KEY_FILE: &str = "node-api-key";
pub(super) const KEY_PREFIX: &str = "sk-fem-";
const TMP_FILE: &str = "node-api-key.tmp";

/// Serialises first-time key creation: `start()` can run concurrently, and two
/// callers must not mint different keys.
static KEY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Exactly `KEY_PREFIX` followed by 64 lowercase hex characters.
pub(super) fn is_valid_key(s: &str) -> bool {
    s.strip_prefix(KEY_PREFIX).is_some_and(|h| {
        h.len() == 64
            && h.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn read_valid(dir: &Path) -> Option<String> {
    let s = std::fs::read_to_string(dir.join(KEY_FILE)).ok()?;
    let t = s.trim();
    is_valid_key(t).then(|| t.to_string())
}

/// Return the persisted key, minting and persisting one if absent or invalid.
/// A valid key is never overwritten; the replacement is an atomic rename.
pub(super) fn load_or_create_key(dir: &Path) -> anyhow::Result<String> {
    let _guard = KEY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(k) = read_valid(dir) {
        return Ok(k);
    }
    let path = dir.join(KEY_FILE);
    if path.exists() {
        warn!("Diiisco: node API key file is invalid — generating a new one");
    }
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let key = format!("{}{}", KEY_PREFIX, hex::encode(bytes));

    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(TMP_FILE);
    write_private(&tmp, &key)?;
    std::fs::rename(&tmp, &path)?;
    Ok(key)
}

/// Write `content` to `path`, owner-only on unix from the moment of creation.
fn write_private(path: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(content.as_bytes())?;
    f.sync_all()
}

/// The persisted key if it is valid; never creates one.
pub(super) fn read_key(dir: &Path) -> Option<String> {
    read_valid(dir)
}

/// Fill the credential's bearer from the device key when neither the env
/// override nor the served field provides one.
pub(super) fn fill_local_key(
    creds: &mut CredentialInfo,
    env: Option<String>,
    dir: &Path,
) -> anyhow::Result<()> {
    if super::resolve_diiisco_bearer(env, creds).is_some() {
        return Ok(());
    }
    creds.diiisco_bearer_token = Some(load_or_create_key(dir)?);
    info!("Diiisco: using this device's own node API key");
    Ok(())
}

#[cfg(test)]
#[path = "diiisco_node_key_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "diiisco_rebuild_tests.rs"]
mod rebuild_tests;

#[cfg(test)]
#[path = "diiisco_k38_tests.rs"]
mod k38_tests;

#[cfg(test)]
#[path = "diiisco_start_gate_tests.rs"]
mod start_gate_tests;

#[cfg(test)]
#[path = "diiisco_rc38_tests.rs"]
mod rc38_tests;

#[cfg(test)]
#[path = "diiisco_sweep_tests.rs"]
mod sweep_tests;

#[cfg(test)]
#[path = "diiisco_fresh_backoff_tests.rs"]
mod fresh_backoff_tests;

#[cfg(test)]
#[path = "diiisco_a4_tests.rs"]
mod a4_tests;

#[cfg(test)]
#[path = "diiisco_g1_start_reuse_tests.rs"]
mod g1_start_reuse_tests;

#[cfg(test)]
#[path = "diiisco_g6_retry_pause_tests.rs"]
mod g6_retry_pause_tests;
