use crate::api::client::{ApiClient, ApiError};
use crate::api::types::{CredentialInfo, VerifiedStatus};

/// GET /credentials/{miner_key} — look up credentials for a miner
pub async fn lookup(client: &ApiClient, miner_key: &str) -> Result<CredentialInfo, ApiError> {
    client.get(&format!("/credentials/{}", miner_key)).await
}

/// GET /credentials/{miner_key}/verified — get verification + staking status
pub async fn get_verified_status(
    client: &ApiClient,
    miner_key: &str,
) -> Result<VerifiedStatus, ApiError> {
    client
        .get(&format!("/credentials/{}/verified", miner_key))
        .await
}
