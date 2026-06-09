use crate::api::client::{ApiClient, ApiError};
use crate::api::types::{LeaseRequest, LeaseResponse, LeaseStatus};

/// POST /leases — acquire a new lease
pub async fn acquire(
    client: &ApiClient,
    request: &LeaseRequest,
) -> Result<LeaseResponse, ApiError> {
    client
        .post(&format!("/leases/{}", request.miner_key), request)
        .await
}

/// PATCH /leases/{miner_key}/{install_id} — renew an existing lease
pub async fn renew(
    client: &ApiClient,
    request: &LeaseRequest,
) -> Result<LeaseResponse, ApiError> {
    client
        .patch(
            &format!("/leases/{}/{}", request.miner_key, request.install_id),
            request,
        )
        .await
}

/// GET /leases/{miner_key} — check lease status
pub async fn status(client: &ApiClient, miner_key: &str) -> Result<LeaseStatus, ApiError> {
    client.get(&format!("/leases/{}", miner_key)).await
}
