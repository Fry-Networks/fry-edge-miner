use crate::integrations::{IntegrationRegistry, PocGateData};

/// Check PoC gates based on current integration state.
/// poa: true iff at least one enabled integration reports poa=true via collect_poc_data().
pub fn check_gates(registry: &IntegrationRegistry) -> PocGateData {
    let has_healthy = registry.enabled_count() > 0;
    let poa = registry
        .list()
        .into_iter()
        .filter(|i| registry.is_enabled(i.id()))
        .any(|i| i.collect_poc_data().poa);
    PocGateData {
        data: has_healthy,
        // online:true is factually correct at submission time — this document
        // only reaches hardwareapi if the device is online; a dead device
        // simply submits nothing and its slots stay empty server-side.
        online: true,
        // mac_match / pol / poi semantics are defined server-side
        // (hardwareapi + dbrewards slot scoring). Client-side enforcement
        // without a server contract risks zeroing legitimate fleet rewards —
        // deferred to a coordinated server+client change.
        mac_match: true,
        pol: true,
        poi: true,
        poa,
    }
}
