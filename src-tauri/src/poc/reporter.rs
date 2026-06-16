use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use tracing::{info, warn};

use crate::api::client::{ApiClient, ApiError};
use crate::api::types::{ApiIntegrationStatus, ApiPocHardwareDoc, ApiPocSlot, PocDocumentWrapper};
use crate::integrations::{HealthStatus, IntegrationRegistry};
use crate::poc::gates::check_gates;

const SLOTS_PER_DAY: u32 = 144;
const SLOT_INTERVAL_MINUTES: u32 = 10;

/// Calculate the current slot number (0-143)
pub fn current_slot_number() -> u32 {
    let now = Utc::now();
    let minutes_since_midnight = now.hour() * 60 + now.minute();
    (minutes_since_midnight / SLOT_INTERVAL_MINUTES) % SLOTS_PER_DAY
}

use chrono::Timelike;

/// Compute health status for each integration by calling health_check().
///
/// Uses block_in_place + Handle::block_on per-integration to bridge async health_check()
/// through the std::sync::Mutex-guarded registry. Each integration's health check holds the
/// registry lock synchronously (no lock across await). Disabled integrations get Stopped.
///
/// Requires multi-threaded tokio runtime (Tauri 2 default).
pub fn compute_health_map(
    registry: &Arc<Mutex<IntegrationRegistry>>,
) -> HashMap<String, HealthStatus> {
    let ids_and_enabled: Vec<(String, bool)> = {
        let reg = registry.lock().unwrap();
        reg.list()
            .iter()
            .map(|i| {
                let id = i.id().to_string();
                let enabled = reg.is_enabled(&id);
                (id, enabled)
            })
            .collect()
    };

    let mut map = HashMap::new();
    for (id, enabled) in &ids_and_enabled {
        if !*enabled {
            map.insert(id.clone(), HealthStatus::Stopped);
            continue;
        }
        let health = tokio::task::block_in_place(|| {
            let reg = registry.lock().unwrap();
            match reg.get(id) {
                Some(integration) => tokio::runtime::Handle::current()
                    .block_on(integration.health_check()),
                None => HealthStatus::Unknown,
            }
        });
        map.insert(id.clone(), health);
    }
    map
}

/// Build a PocHardwareDoc from current registry state and pre-computed health.
///
/// Reward scalars (proportion, multiplier, per-integration healthy) use Healthy-only counting.
/// Display fields (active_count, tools_count, tools_active, enabled) remain enabled-based.
pub fn build_poc_doc(
    miner_key: &str,
    registry: &IntegrationRegistry,
    health_map: &HashMap<String, HealthStatus>,
) -> ApiPocHardwareDoc {
    let gates = check_gates(registry);
    let slot_number = current_slot_number();

    let mut integrations = HashMap::new();
    for integration in registry.list() {
        let id = integration.id().to_string();
        let enabled = registry.is_enabled(&id);
        let health = health_map.get(&id).cloned().unwrap_or(HealthStatus::Unknown);
        let healthy = matches!(health, HealthStatus::Healthy);
        integrations.insert(
            id,
            ApiIntegrationStatus {
                enabled,
                healthy,
                version: None,
            },
        );
    }

    // Reward scalars — Healthy-only
    let healthy_count = health_map
        .values()
        .filter(|s| matches!(s, HealthStatus::Healthy))
        .count() as u32;
    let total_count = registry.total_count();
    let proportion = if total_count == 0 {
        0.0
    } else {
        healthy_count as f64 / total_count as f64
    };

    // Display fields — enabled-based (unchanged)
    let active_count = registry.enabled_count();
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
#[allow(dead_code)] // Phase 3: async PoC submission
pub async fn submit_poc(
    client: &ApiClient,
    miner_key: &str,
    registry: &Arc<Mutex<IntegrationRegistry>>,
) -> Result<(), ApiError> {
    let health_map = compute_health_map(registry);
    let doc = {
        let reg = registry.lock().unwrap();
        build_poc_doc(miner_key, &reg, &health_map)
    };
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
