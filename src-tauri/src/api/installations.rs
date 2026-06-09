use crate::api::client::{ApiClient, ApiError};
use crate::api::types::{InstallationRequest, InstallationResponse};

/// POST /installations — register a new installation
pub async fn register(
    client: &ApiClient,
    request: &InstallationRequest,
) -> Result<InstallationResponse, ApiError> {
    client
        .post(&format!("/installations/{}", request.miner_key), request)
        .await
}

/// DELETE /installations/{miner_key}/{install_id} — unregister
pub async fn unregister(
    client: &ApiClient,
    miner_key: &str,
    install_id: &str,
) -> Result<(), ApiError> {
    client
        .delete(&format!("/installations/{}/{}", miner_key, install_id))
        .await
}
