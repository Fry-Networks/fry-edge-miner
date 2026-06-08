pub mod reporter;
pub mod gates;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PocSlot {
    pub slot_index: u32,
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
pub struct IntegrationPocData {
    pub enabled: bool,
    pub healthy: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PocHardwareDoc {
    pub miner_key: String,
    pub miner_type: String,
    pub integrations: HashMap<String, IntegrationPocData>,
    pub active_count: u32,
    pub total_count: u32,
    pub proportion: f64,
    pub slots: Vec<PocSlot>,
}
