use serde::Serialize;

use crate::api::client::{ApiClient, ApiError};

#[derive(Debug, Serialize)]
struct MigrationRequest<'a> {
    fem_key: &'a str,
    old_keys: &'a [String],
}

/// POST /devices/migrate — notify hardwareapi to deactivate old miner keys.
/// Non-fatal: if the call fails (401, network error, etc.), the caller logs a warning
/// and migration continues — the local config change already succeeded.
pub async fn notify_migration(
    client: &ApiClient,
    fem_key: &str,
    old_keys: &[String],
) -> Result<(), ApiError> {
    let body = MigrationRequest { fem_key, old_keys };
    client
        .post::<serde_json::Value, _>("/devices/migrate", &body)
        .await?;
    Ok(())
}
