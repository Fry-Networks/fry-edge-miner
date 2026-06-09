use crate::integrations::{IntegrationRegistry, PocGateData};

/// Check PoC gates based on current integration state.
/// v1: data=true if any integration healthy, online/mac_match/pol/poi/poa default true.
pub fn check_gates(registry: &IntegrationRegistry) -> PocGateData {
    let has_healthy = registry.enabled_count() > 0;
    PocGateData {
        data: has_healthy,
        online: true,
        mac_match: true,
        pol: true,
        poi: true,
        poa: true,
    }
}
