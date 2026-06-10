use crate::api::client::{ApiClient, ApiError};
use crate::api::types::{GenericOk, InstallationHeartbeat};

/// POST /installations/{miner_key}/installations/{install_id} — upsert installation heartbeat
pub async fn register(
    client: &ApiClient,
    request: &InstallationHeartbeat,
) -> Result<GenericOk, ApiError> {
    client
        .post(
            &format!(
                "/installations/{}/installations/{}",
                request.miner_key, request.install_id
            ),
            request,
        )
        .await
}

/// DELETE /installations/{miner_key}/installations/{install_id} — remove installation
pub async fn unregister(
    client: &ApiClient,
    miner_key: &str,
    install_id: &str,
) -> Result<(), ApiError> {
    client
        .delete(&format!(
            "/installations/{}/installations/{}",
            miner_key, install_id
        ))
        .await
}
