/// Pawns.app Integration — Partner-Gated SDK Stub
///
/// The Pawns SDK is partner-sales gated and requires explicit user consent before enabling.
/// Users must opt-in to the bandwidth sharing terms which disclose $0.20/GB residential bandwidth pricing.
/// This module provides a complete lifecycle implementation with honest fail-closed gates:
///
/// 1. **User Consent Gate** (future state): Requires environment variable `PAWNS_USER_CONSENT=accepted`.
///    Discloses: "Pawns bandwidth sharing requires explicit user consent ($0.20/GB residential bandwidth disclosure)".
/// 2. **SDK Key Gate** (future state): Requires `PAWNS_SDK_KEY` to be provisioned.
///    Fails closed if either gate is not satisfied.
///
/// Both gates are mandatory per Pawns SDK terms. This module never fakes a healthy state —
/// until both gates are satisfied, health checks return Unhealthy with a clear reason.
///

use super::{HealthStatus, Integration, PocGateData};
use anyhow::Result;
use async_trait::async_trait;
use tracing::info;

pub struct PawnsIntegration;

impl PawnsIntegration {
    fn user_consent() -> bool {
        std::env::var("PAWNS_USER_CONSENT")
            .map(|v| v.to_lowercase() == "accepted")
            .unwrap_or(false)
    }

    fn sdk_key() -> Option<String> {
        std::env::var("PAWNS_SDK_KEY")
            .ok()
            .filter(|s| !s.is_empty())
    }
}

#[async_trait]
impl Integration for PawnsIntegration {
    fn id(&self) -> &str {
        "pawns"
    }

    fn display_name(&self) -> &str {
        "Pawns.app"
    }

    async fn install(&self) -> Result<()> {
        // No-op install — the Pawns SDK is not yet available.
        // This placeholder documents the future integration point.
        info!("Pawns SDK pending partner onboarding — install is a no-op in this release");
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        // Gate 1: User consent
        if !Self::user_consent() {
            anyhow::bail!(
                "Pawns bandwidth sharing requires explicit user consent ($0.20/GB residential bandwidth disclosure). \
                 Set environment variable PAWNS_USER_CONSENT=accepted to proceed."
            );
        }

        // Gate 2: SDK key
        if Self::sdk_key().is_none() {
            anyhow::bail!(
                "Pawns SDK key pending partner approval. \
                 Once approved, set the environment variable PAWNS_SDK_KEY to enable this integration."
            );
        }

        // Both gates satisfied, but SDK is not yet available for delivery
        anyhow::bail!(
            "Pawns SDK integration pending partner binary delivery. \
             Please contact Pawns support to obtain the latest SDK."
        );
    }

    async fn stop(&self) -> Result<()> {
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        // Always report Unhealthy in this release — the SDK is not available yet
        if Self::sdk_key().is_none() {
            return HealthStatus::Unhealthy("Pawns SDK key pending partner approval".to_string());
        }
        HealthStatus::Unhealthy("Pawns SDK integration pending partner binary delivery".to_string())
    }

    async fn check_update(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn installed_version(&self) -> Option<String> {
        None
    }

    fn collect_poc_data(&self) -> PocGateData {
        PocGateData {
            poa: false,
            ..Default::default()
        }
    }

    fn requires_docker(&self) -> bool {
        false
    }
}
