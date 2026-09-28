//! D-C6-4: two setup texts are AWAITING_MARKERS literals of their own, so the
//! supervisor treats them as a step only the user can take, not a fault to
//! restart into.
//!
//! The firewall line reaches the card behind the "Awaiting administrator
//! action" marker (fryvpn::firewall_setup_reason), which already classifies it.
//! These tests use the BARE sentence so they depend on the new literal alone.

use super::{awaits_user_action, fryvpn, space_acres, AWAITING_MARKERS};
use crate::integrations::HealthStatus;
use crate::supervisor::health::{pause_reason, recovery_action, HealthCheckConfig, RecoveryAction};

fn action_for(reason: &str) -> RecoveryAction {
    recovery_action(&HealthStatus::Unhealthy(reason.to_string()), true, 0, 6)
}

#[test]
fn a_bare_firewall_setup_line_awaits_the_user_and_is_never_restarted() {
    let bare = fryvpn::FIREWALL_SETUP_TEXT;
    let with_shortfall = fryvpn::join_shortfall(
        bare.to_string(),
        Some("registration needs 0.2 ALGO more".to_string()),
    );
    for reason in [bare, with_shortfall.as_str()] {
        assert!(awaits_user_action(reason), "{reason}");
        assert_eq!(action_for(reason), RecoveryAction::None, "{reason}");
    }
}

#[test]
fn the_firewall_literal_is_pinned_to_its_source_constant() {
    assert!(AWAITING_MARKERS.contains(&fryvpn::FIREWALL_SETUP_TEXT));
}

#[test]
fn the_spaceacres_setup_reason_awaits_the_user_and_is_never_restarted() {
    let plain = space_acres::SETUP_REQUIRED_REASON;
    let paused = pause_reason(plain, &HealthCheckConfig::default());
    for reason in [plain, paused.as_str()] {
        assert!(awaits_user_action(reason), "{reason}");
        assert_eq!(action_for(reason), RecoveryAction::None, "{reason}");
    }
}

#[test]
fn the_spaceacres_literal_is_pinned_to_its_source_constant() {
    assert!(AWAITING_MARKERS.contains(&space_acres::SETUP_REQUIRED_REASON));
}

/// Controls: ordinary faults of the same two partners still restart.
#[test]
fn ordinary_frynode_and_spaceacres_faults_still_restart() {
    for reason in [
        "frynode process is not running",
        "SpaceAcres process is not running",
        "Finish setup in the SpaceAcres window",
        "Error detected in daemon logs",
    ] {
        assert!(!awaits_user_action(reason), "{reason}");
        assert_eq!(action_for(reason), RecoveryAction::Restart, "{reason}");
    }
}
