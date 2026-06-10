use std::collections::HashMap;

use chrono::Utc;
use tracing::{info, warn};

use crate::api::client::{ApiClient, ApiError};
use crate::api::types::{ApiIntegrationStatus, ApiPocHardwareDoc, ApiPocSlot, PocDocumentWrapper};
use crate::integrations::IntegrationRegistry;
use crate::poc::gates::check_gates;

const SLOTS_PER_DAY: u32 = 144;
const SLOT_INTERVAL_MINUTES: u32 = 10;

/// Calculate the current slot number (0-143)
pub fn current_slot_number() -> u32 {
    let now = Utc::now();
    let minutes_since_midnight = (now.hour() * 60 + now.minute()) as u32;
    (minutes_since_midnight / SLOT_INTERVAL_MINUTES) % SLOTS_PER_DAY
}

use chrono::Timelike;

/// Build a PocHardwareDoc from current registry state
pub fn build_poc_doc(
    miner_key: &str,
    registry: &IntegrationRegistry,
) -> ApiPocHardwareDoc {
    let gates = check_gates(registry);
    let slot_number = current_slot_number();

    let mut integrations = HashMap::new();
    for integration in registry.list() {
        let enabled = registry.is_enabled(integration.id());
        integrations.insert(
            integration.id().to_string(),
            ApiIntegrationStatus {
                enabled,
                healthy: enabled, // stub: healthy if enabled
                version: None,
            },
        );
    }

    let active_count = registry.enabled_count();
    let total_count = registry.total_count();
    let proportion = registry.proportion();

    let active_tools: Vec<String> = registry
        .list()
        .iter()
        .filter(|i| registry.is_enabled(i.id()))
        .map(|i| i.id().to_string())
        .collect();

    let slot = ApiPocSlot {
        slot_number,
        data: gates.data,
        online: gates.online,
        mac_match: gates.mac_match,
        pol: gates.pol,
        poi: gates.poi,
        poa: gates.poa,
        tools_active: active_tools,
        tools_count: active_count,
        multiplier: proportion,
    };

    ApiPocHardwareDoc {
        miner_key: miner_key.to_string(),
        miner_type: "FEM".to_string(),
        integrations,
        active_count,
        total_count,
        proportion,
        slots: vec![slot],
    }
}

/// Submit PoC data to hardwareapi
pub async fn submit_poc(
    client: &ApiClient,
    miner_key: &str,
    registry: &IntegrationRegistry,
) -> Result<(), ApiError> {
    let doc = build_poc_doc(miner_key, registry);
    let slot = current_slot_number();

    info!(
        miner_key = miner_key,
        slot = slot,
        proportion = doc.proportion,
        active = doc.active_count,
        total = doc.total_count,
        "Submitting PoC data"
    );

    let wrapped = PocDocumentWrapper { document: doc };
    match client
        .put_json(&format!("/PoC/{}/hardware", miner_key), &wrapped)
        .await
    {
        Ok(()) => {
            info!(miner_key = miner_key, slot = slot, "PoC submitted");
            Ok(())
        }
        Err(e) => {
            warn!(miner_key = miner_key, error = %e, "PoC submission failed");
            Err(e)
        }
    }
}
