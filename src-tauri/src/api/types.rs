use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// --- Version ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub software_version_needed: Option<String>,
    pub poc_version_needed: Option<String>,
    pub limit: Option<u32>,
    pub multiplier_base: Option<f64>,
    pub multiplier_per_tool: Option<f64>,
}

// --- Installation ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallationRequest {
    pub miner_key: String,
    pub install_id: String,
    pub software_version: String,
    pub poc_version: String,
    pub platform: String,
    pub mac_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallationResponse {
    pub success: bool,
    pub message: Option<String>,
}

// --- Lease ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseRequest {
    pub miner_key: String,
    pub install_id: String,
    pub lease_seconds: u64,
    pub external_ip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseResponse {
    pub granted: bool,
    pub expires_at: Option<String>,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseStatus {
    pub active: bool,
    pub miner_key: Option<String>,
    pub install_id: Option<String>,
    pub expires_at: Option<String>,
}

// --- Credentials ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialInfo {
    pub miner_key: String,
    pub credentials: HashMap<String, serde_json::Value>,
}

// --- IP Status ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpStatus {
    pub external_ip: String,
    pub installations_by_type: HashMap<String, u32>,
}

// --- Supported Miners ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupportedMiner {
    pub code: String,
    pub name: String,
    pub category: String,
}

// --- PoC Hardware (API-facing) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiPocSlot {
    pub slot_number: u32,
    pub data: bool,
    pub online: bool,
    pub mac_match: bool,
    pub pol: bool,
    pub poi: bool,
    pub poa: bool,
    pub tools_active: Vec<String>,
    pub tools_count: u32,
    pub multiplier: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiIntegrationStatus {
    pub enabled: bool,
    pub healthy: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiPocHardwareDoc {
    pub miner_key: String,
    pub miner_type: String,
    pub integrations: HashMap<String, ApiIntegrationStatus>,
    pub active_count: u32,
    pub total_count: u32,
    pub proportion: f64,
    pub slots: Vec<ApiPocSlot>,
}

// --- Miner Profile ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinerProfile {
    pub miner_key: String,
    #[serde(default)]
    pub installations: Vec<serde_json::Value>,
}
