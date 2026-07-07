use std::sync::{Arc, RwLock};
use std::time::Duration;

use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("HTTP {0}: {1}")]
    HttpStatus(u16, String),
    #[error("Failed to decode response from {path} (HTTP {status}): {message} — body starts with: {snippet}")]
    Decode {
        path: String,
        status: u16,
        message: String,
        snippet: String,
    },
}

/// Parse a JSON body with actionable context instead of reqwest's bare
/// "error decoding response body".
fn decode_json<T: DeserializeOwned>(path: &str, status: u16, body: String) -> Result<T, ApiError> {
    serde_json::from_str::<T>(&body).map_err(|e| ApiError::Decode {
        path: path.to_string(),
        status,
        message: e.to_string(),
        snippet: body.chars().take(300).collect(),
    })
}

impl serde::Serialize for ApiError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub struct ApiClient {
    pub(crate) base_url: String,
    pub(crate) bearer_token: Arc<RwLock<String>>,
    pub(crate) http: Client,
}

impl ApiClient {
    pub fn new(base_url: String, bearer_token: String) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            base_url,
            bearer_token: Arc::new(RwLock::new(bearer_token)),
            http,
        }
    }

    pub fn set_bearer_token(&self, token: String) {
        *self.bearer_token.write().unwrap() = token;
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let token = self.bearer_token.read().unwrap().clone();
        let mut req = self.http.get(self.url(path));
        if !token.is_empty() {
            req = req.bearer_auth(&token);
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiError::HttpStatus(status.as_u16(), body));
        }
        let body = resp.text().await?;
        decode_json(path, status.as_u16(), body)
    }

    pub async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let token = self.bearer_token.read().unwrap().clone();
        let mut req = self.http.post(self.url(path)).json(body);
        if !token.is_empty() {
            req = req.bearer_auth(&token);
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiError::HttpStatus(status.as_u16(), body));
        }
        let body = resp.text().await?;
        decode_json(path, status.as_u16(), body)
    }

    pub async fn put_json<B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<(), ApiError> {
        let token = self.bearer_token.read().unwrap().clone();
        let mut req = self.http.put(self.url(path)).json(body);
        if !token.is_empty() {
            req = req.bearer_auth(&token);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiError::HttpStatus(status, body));
        }
        Ok(())
    }

    pub async fn patch<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let token = self.bearer_token.read().unwrap().clone();
        let mut req = self.http.patch(self.url(path)).json(body);
        if !token.is_empty() {
            req = req.bearer_auth(&token);
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiError::HttpStatus(status.as_u16(), body));
        }
        let body = resp.text().await?;
        decode_json(path, status.as_u16(), body)
    }

    pub async fn delete(&self, path: &str) -> Result<(), ApiError> {
        let token = self.bearer_token.read().unwrap().clone();
        let mut req = self.http.delete(self.url(path));
        if !token.is_empty() {
            req = req.bearer_auth(&token);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiError::HttpStatus(status, body));
        }
        Ok(())
    }
}
