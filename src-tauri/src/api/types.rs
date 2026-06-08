use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionResponse {
    pub software_version: Option<String>,
    pub poc_version: Option<String>,
    pub limit: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseResponse {
    pub granted: bool,
    pub expires_at: Option<String>,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpStatusResponse {
    pub external_ip: String,
    pub installations_by_type: serde_json::Value,
}
