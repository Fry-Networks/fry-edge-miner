use crate::api::client::ApiClient;
use crate::api::types::RewardConfig;

pub async fn get_product(client: &ApiClient, key: &str) -> Result<RewardConfig, crate::api::client::ApiError> {
    client.get(&format!("/products/{}", key)).await
}
