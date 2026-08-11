use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use tracing::{info, warn};

use crate::api::client::{ApiClient, ApiError};
use crate::api::types::{ApiIntegrationStatus, ApiPocHardwareDoc, ApiPocSlot, ApiSoftwareInfo, PocDocumentWrapper};
use crate::integrations::{HealthStatus, IntegrationRegistry};
use crate::poc::gates::check_gates;

const SLOTS_PER_DAY: u32 = 144;
const SLOT_INTERVAL_MINUTES: u32 = 10;

/// Per-integration ceiling for one health check inside a reporting tick.
/// Must stay below the 60s tick so a slow check delays one report at most.
const HEALTH_CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

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
    compute_health_map_with_timeout(registry, HEALTH_CHECK_TIMEOUT)
}

/// Timeout-parameterised core of [`compute_health_map`] so the stall guard is
/// testable without waiting the production ceiling.
pub fn compute_health_map_with_timeout(
    registry: &Arc<Mutex<IntegrationRegistry>>,
    health_timeout: std::time::Duration,
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
    let mut to_check: Vec<(String, std::sync::Arc<dyn crate::integrations::Integration>)> =
        Vec::new();
    {
        let reg = registry.lock().unwrap();
        for (id, enabled) in &ids_and_enabled {
            if !*enabled {
                map.insert(id.clone(), HealthStatus::Stopped);
                continue;
            }
            match reg.get(id) {
                Some(integration) => to_check.push((id.clone(), integration)),
                None => {
                    map.insert(id.clone(), HealthStatus::Unknown);
                }
            }
        }
    }

    // Second line of defence behind platform::output_bounded: a health check
    // that stalls must degrade ITS integration, never wedge the reporting tick
    // (v0.4.8 froze for 4.5h this way). Checks run CONCURRENTLY so the tick's
    // worst case is one timeout, not N of them serially.
    let results = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let mut tasks = Vec::with_capacity(to_check.len());
            for (id, integration) in to_check {
                tasks.push(tokio::spawn(async move {
                    let status = match tokio::time::timeout(
                        health_timeout,
                        integration.health_check(),
                    )
                    .await
                    {
                        Ok(status) => status,
                        Err(_) => {
                            warn!(
                                integration = id.as_str(),
                                secs = health_timeout.as_secs(),
                                "health check timed out"
                            );
                            HealthStatus::Unhealthy("health check timed out".to_string())
                        }
                    };
                    (id, status)
                }));
            }
            let mut out = Vec::with_capacity(tasks.len());
            for task in tasks {
                match task.await {
                    Ok(pair) => out.push(pair),
                    Err(e) => warn!(error = %e, "health check task failed"),
                }
            }
            out
        })
    });
    for (id, status) in results {
        map.insert(id, status);
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
    // Denominator is what this machine can actually run, not everything in the
    // registry. Dividing by total_count docks users for integrations their
    // hardware rules out, so shipping one nobody can run would quietly cut
    // every user's multiplier.
    let available_count = registry.available_count();
    let proportion = if available_count == 0 {
        0.0
    } else {
        healthy_count as f64 / available_count as f64
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
        // Same constants the installation heartbeat reports (commands/device.rs).
        software: Some(ApiSoftwareInfo {
            os: std::env::consts::OS.to_string(),
            poc_version_installed: "1.0.0".to_string(),
            software_version_installed: env!("CARGO_PKG_VERSION").to_string(),
        }),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::{Integration, PocGateData};
    use anyhow::Result;
    use async_trait::async_trait;

    /// Minimal integration whose availability we control, so the denominator
    /// can be tested without touching real disks or partner binaries.
    struct FakeIntegration {
        id: &'static str,
        blocked: Option<&'static str>,
    }

    #[async_trait]
    impl Integration for FakeIntegration {
        fn id(&self) -> &str {
            self.id
        }
        fn display_name(&self) -> &str {
            self.id
        }
        async fn install(&self) -> Result<()> {
            Ok(())
        }
        async fn start(&self) -> Result<()> {
            Ok(())
        }
        async fn stop(&self) -> Result<()> {
            Ok(())
        }
        async fn health_check(&self) -> HealthStatus {
            HealthStatus::Healthy
        }
        async fn check_update(&self) -> Result<Option<String>> {
            Ok(None)
        }
        fn collect_poc_data(&self) -> PocGateData {
            PocGateData::default()
        }
        fn check_requirements(&self) -> Result<(), String> {
            match self.blocked {
                Some(reason) => Err(reason.to_string()),
                None => Ok(()),
            }
        }
    }

    fn registry_with(specs: &[(&'static str, Option<&'static str>)]) -> IntegrationRegistry {
        let mut reg = IntegrationRegistry::new();
        for (id, blocked) in specs {
            reg.register(Arc::new(FakeIntegration {
                id,
                blocked: *blocked,
            }));
        }
        reg
    }

    fn healthy_map(ids: &[&str]) -> HashMap<String, HealthStatus> {
        ids.iter()
            .map(|id| (id.to_string(), HealthStatus::Healthy))
            .collect()
    }

    #[test]
    fn available_count_excludes_integrations_this_machine_cannot_run() {
        let reg = registry_with(&[
            ("a", None),
            ("b", None),
            ("c", Some("needs 900 GB")),
        ]);
        assert_eq!(reg.total_count(), 3);
        assert_eq!(reg.available_count(), 2);
    }

    #[test]
    fn proportion_divides_by_available_not_total() {
        // Two healthy out of three registered, one of which this machine can
        // never run. Dividing by total would report 0.667 and quietly dock the
        // user for hardware they do not have; the honest figure is 1.0.
        let reg = registry_with(&[
            ("a", None),
            ("b", None),
            ("c", Some("needs 900 GB")),
        ]);
        let doc = build_poc_doc("FEM-TEST", &reg, &healthy_map(&["a", "b"]));
        assert_eq!(doc.proportion, 1.0, "got {}", doc.proportion);
        assert_eq!(doc.slots[0].multiplier, 1.0);
    }

    #[test]
    fn proportion_is_unchanged_when_everything_is_runnable() {
        let reg = registry_with(&[("a", None), ("b", None), ("c", None), ("d", None)]);
        let doc = build_poc_doc("FEM-TEST", &reg, &healthy_map(&["a"]));
        assert_eq!(doc.proportion, 0.25);
    }

    #[test]
    fn all_unavailable_yields_zero_not_a_divide_by_zero() {
        let reg = registry_with(&[("a", Some("dead")), ("b", Some("dead"))]);
        let doc = build_poc_doc("FEM-TEST", &reg, &HashMap::new());
        assert_eq!(doc.proportion, 0.0);
    }

    #[test]
    fn poc_doc_reports_the_software_version_block() {
        // The server eligibility gate fail-closes without software.poc_version_installed;
        // v0.2.19..v0.4.8 omitted the block and zeroed FEM rewards fleet-wide.
        let reg = registry_with(&[("a", None)]);
        let doc = build_poc_doc("FEM-TEST", &reg, &healthy_map(&["a"]));
        let software = doc.software.expect("software block must be present");
        assert_eq!(software.poc_version_installed, "1.0.0");
        assert_eq!(software.software_version_installed, env!("CARGO_PKG_VERSION"));
        assert!(!software.os.is_empty());
    }

    #[test]
    fn total_count_still_reports_the_whole_registry() {
        // total_count remains the registry size — only the denominator moved.
        let reg = registry_with(&[("a", None), ("b", Some("needs 900 GB"))]);
        let doc = build_poc_doc("FEM-TEST", &reg, &healthy_map(&["a"]));
        assert_eq!(doc.total_count, 2);
    }
}

#[cfg(test)]
mod health_timeout_tests {
    use super::*;
    use crate::integrations::{Integration, PocGateData};
    use anyhow::Result;
    use async_trait::async_trait;

    /// Integration whose health check never returns — models a docker CLI call
    /// wedged on a dead daemon (v0.4.8 froze the reporting tick for 4.5h).
    struct HangingIntegration;

    #[async_trait]
    impl Integration for HangingIntegration {
        fn id(&self) -> &str { "hanger" }
        fn display_name(&self) -> &str { "hanger" }
        async fn install(&self) -> Result<()> { Ok(()) }
        async fn start(&self) -> Result<()> { Ok(()) }
        async fn stop(&self) -> Result<()> { Ok(()) }
        async fn health_check(&self) -> HealthStatus {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            HealthStatus::Healthy
        }
        async fn check_update(&self) -> Result<Option<String>> { Ok(None) }
        fn collect_poc_data(&self) -> PocGateData { PocGateData::default() }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn hanging_health_check_degrades_that_integration_only() {
        let mut reg = IntegrationRegistry::new();
        reg.register(Arc::new(HangingIntegration));
        reg.set_enabled("hanger", true);
        let registry = Arc::new(Mutex::new(reg));

        let started = std::time::Instant::now();
        let map = tokio::task::spawn_blocking(move || {
            compute_health_map_with_timeout(&registry, std::time::Duration::from_millis(200))
        })
        .await
        .expect("compute_health_map must return, not hang");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "took {:?} — stall guard did not fire",
            started.elapsed()
        );

        match map.get("hanger") {
            Some(HealthStatus::Unhealthy(reason)) => {
                assert!(reason.contains("timed out"), "got {reason}")
            }
            other => panic!("expected Unhealthy(timed out), got {other:?}"),
        }
    }
}

#[cfg(test)]
mod health_concurrency_tests {
    use super::*;
    use crate::integrations::{Integration, PocGateData};
    use anyhow::Result;
    use async_trait::async_trait;

    /// Health check that always outlives the ceiling.
    struct SlowIntegration(&'static str);

    #[async_trait]
    impl Integration for SlowIntegration {
        fn id(&self) -> &str { self.0 }
        fn display_name(&self) -> &str { self.0 }
        async fn install(&self) -> Result<()> { Ok(()) }
        async fn start(&self) -> Result<()> { Ok(()) }
        async fn stop(&self) -> Result<()> { Ok(()) }
        async fn health_check(&self) -> HealthStatus {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            HealthStatus::Healthy
        }
        async fn check_update(&self) -> Result<Option<String>> { Ok(None) }
        fn collect_poc_data(&self) -> PocGateData { PocGateData::default() }
    }

    /// Serial evaluation would cost N × timeout; concurrent costs ~1 × timeout.
    /// Guards the reviewer's "10 integrations × 45s = 450s still wedges the
    /// tick" finding.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn stalled_checks_do_not_stack_across_integrations() {
        let mut reg = IntegrationRegistry::new();
        for id in ["a", "b", "c", "d", "e", "f"] {
            reg.register(Arc::new(SlowIntegration(id)));
            reg.set_enabled(id, true);
        }
        let registry = Arc::new(Mutex::new(reg));
        let per_check = std::time::Duration::from_millis(500);

        let started = std::time::Instant::now();
        let map = tokio::task::spawn_blocking(move || {
            compute_health_map_with_timeout(&registry, per_check)
        })
        .await
        .expect("must return");
        let elapsed = started.elapsed();

        assert_eq!(map.len(), 6);
        assert!(
            map.values().all(|s| matches!(s, HealthStatus::Unhealthy(r) if r.contains("timed out"))),
            "all six should time out: {map:?}"
        );
        // Serial would be ~3s (6 × 500ms); concurrent stays near one timeout.
        assert!(
            elapsed < per_check * 3,
            "took {elapsed:?} for 6 stalled checks — evaluation is not concurrent"
        );
    }
}
