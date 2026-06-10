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

// --- Installation (matches server InstallationHeartbeat, models.py) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallationHeartbeat {
    pub miner_key: String,
    pub install_id: String,
    #[serde(rename = "minerCode", skip_serializing_if = "Option::is_none")]
    pub miner_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_version_installed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poc_version_installed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_installed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericOk {
    pub ok: bool,
}

// --- Lease (matches server LeaseAction + LeaseResponse, models.py) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseAction {
    pub lease_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_ip: Option<String>,
}

impl Default for LeaseAction {
    fn default() -> Self {
        Self { lease_seconds: 900, external_ip: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseResponse {
    pub granted: bool,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub holder_install_id: Option<String>,
    #[serde(default)]
    pub ttl_seconds: Option<i64>,
    #[serde(default)]
    pub error_code: Option<String>,
}

// --- PoC Document Wrapper (matches server HardwareDocument, models.py:341) ---

#[derive(Debug, Serialize)]
pub struct PocDocumentWrapper {
    pub document: ApiPocHardwareDoc,
}

// --- Credentials ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialInfo {
    pub miner_key: String,
    pub credentials: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub algo_address: Option<String>,
    #[serde(default)]
    pub algo_mnemonic: Option<String>,
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
