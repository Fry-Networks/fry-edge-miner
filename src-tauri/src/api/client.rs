use std::sync::{Arc, RwLock};
use std::time::Duration;

use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("{}", format_http_status(*.0, .1))]
    HttpStatus(u16, String),
    #[error("Failed to decode response from {path} (HTTP {status}): {message} — body starts with: {snippet}")]
    Decode {
        path: String,
        status: u16,
        message: String,
        snippet: String,
    },
}

/// User-facing form of an HTTP error: "HTTP 504 Gateway Timeout: <safe body>".
fn format_http_status(status: u16, body: &str) -> String {
    let reason = status_reason(status);
    if reason.is_empty() {
        format!("HTTP {status}: {}", displayable_body(body))
    } else {
        format!("HTTP {status} {reason}: {}", displayable_body(body))
    }
}

/// Human-readable reason for the status codes hardwareapi's edge actually
/// produces; empty for anything else so the code alone still reads fine.
fn status_reason(status: u16) -> &'static str {
    match status {
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

/// UB-3: what an error body is allowed to look like in a user-facing message.
///
/// A CDN or proxy failure hands back its whole HTML error page; rendering
/// that verbatim put a full `<!DOCTYPE html>` dump in the Dashboard banner.
/// The raw body stays in the variant for callers that pattern-match on it —
/// only Display is filtered.
fn displayable_body(body: &str) -> String {
    const MAX_BODY_CHARS: usize = 200;
    let trimmed = body.trim();
    if trimmed.starts_with('<') {
        return "(HTML error page from server/CDN suppressed)".to_string();
    }
    if trimmed.chars().count() > MAX_BODY_CHARS {
        let cut: String = trimmed.chars().take(MAX_BODY_CHARS).collect();
        return format!("{cut}…");
    }
    trimmed.to_string()
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
            .connect_timeout(Duration::from_secs(10))
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

/// UB-3: the Dashboard banner renders `ApiError` Display verbatim, and a CDN
/// 504 hands back its whole HTML error page as the body — which used to be
/// dumped raw into the UI.
#[cfg(test)]
mod http_status_display_tests {
    use super::*;

    const BUNNY_504_PAGE: &str = r#"<!DOCTYPE html>
<html>
<head><title>504 Gateway Timeout</title>
<script src="https://bunnynetassets.b-cdn.net/x.js"></script></head>
<body>We can't connect to the server at 124.190.73.241.</body>
</html>"#;

    #[test]
    fn an_html_error_body_is_never_shown_verbatim() {
        let msg = ApiError::HttpStatus(504, BUNNY_504_PAGE.to_string()).to_string();
        assert!(
            !msg.contains("<!DOCTYPE") && !msg.contains("<html"),
            "HTML error page leaked into the user-facing message: {msg}"
        );
        assert!(msg.contains("504"), "status code must survive: {msg}");
        assert!(
            msg.contains("Gateway Timeout"),
            "the human-readable reason should replace the page: {msg}"
        );
    }

    #[test]
    fn a_short_json_error_body_still_shows_its_detail() {
        let msg = ApiError::HttpStatus(409, r#"{"detail":"IP conflict"}"#.to_string()).to_string();
        assert!(msg.contains("IP conflict"), "API JSON detail must survive: {msg}");
        assert!(msg.contains("409"), "{msg}");
    }

    #[test]
    fn an_oversized_text_body_is_truncated() {
        let long = "x".repeat(1000);
        let msg = ApiError::HttpStatus(500, long).to_string();
        assert!(msg.len() < 400, "body must be truncated, got {} chars", msg.len());
    }
}
