use crate::api::client::{ApiClient, ApiError};
use crate::api::types::{LeaseAction, LeaseResponse};

/// POST /installations/{miner_key}/leases/{install_id} — acquire a mining lease
pub async fn acquire(
    client: &ApiClient,
    miner_key: &str,
    install_id: &str,
    action: &LeaseAction,
) -> Result<LeaseResponse, ApiError> {
    client
        .post(
            &format!("/installations/{}/leases/{}", miner_key, install_id),
            action,
        )
        .await
}

/// PATCH /installations/{miner_key}/leases/{install_id} — renew an existing lease
pub async fn renew(
    client: &ApiClient,
    miner_key: &str,
    install_id: &str,
    action: &LeaseAction,
) -> Result<LeaseResponse, ApiError> {
    client
        .patch(
            &format!("/installations/{}/leases/{}", miner_key, install_id),
            action,
        )
        .await
}

/// GET /installations/{miner_key}/leases/current — check lease status
pub async fn status(client: &ApiClient, miner_key: &str) -> Result<LeaseResponse, ApiError> {
    client
        .get(&format!("/installations/{}/leases/current", miner_key))
        .await
}
