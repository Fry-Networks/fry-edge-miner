use crate::api::client::{ApiClient, ApiError};
use crate::api::types::VersionInfo;

/// GET /versions/{code}?platform={platform}
pub async fn check_version(
    client: &ApiClient,
    code: &str,
    platform: &str,
) -> Result<VersionInfo, ApiError> {
    client
        .get(&format!("/versions/{}?platform={}", code, platform))
        .await
}
