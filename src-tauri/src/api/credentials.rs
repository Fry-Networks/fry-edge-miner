use crate::api::client::{ApiClient, ApiError};
use crate::api::types::CredentialInfo;

/// GET /credentials/{miner_key} — look up credentials for a miner
pub async fn lookup(client: &ApiClient, miner_key: &str) -> Result<CredentialInfo, ApiError> {
    client.get(&format!("/credentials/{}", miner_key)).await
}
