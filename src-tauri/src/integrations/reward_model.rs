//! The FEM integration reward model.
//!
//! One required integration active earns the FULL base reward; the second one
//! is the largest single boost available. Optional integrations earn their
//! boost whether or not a required integration is running, so a device with no
//! required integration still earns `base × boost × multipliers`.
//!
//! These constants are mirrored in two other places and must be changed in all
//! three together:
//!   * dbrewards `src/reward.ts` (`computeFemIntegrationTier`) — AUTHORITATIVE.
//!     The server recomputes every count from the per-integration health data
//!     in the PoC document and never trusts a client-supplied proportion.
//!   * the dashboard's FEM reward display (HERMES00) — display only.
//! This module is the FEM client's copy and drives the on-device estimate only.

/// Boost for running BOTH required integrations rather than one.
pub const SECOND_REQUIRED_BOOST: f64 = 0.15;
/// Boost per healthy official-partner integration (excludes the required two).
pub const OFFICIAL_PARTNER_BOOST: f64 = 0.10;
/// Boost per healthy community/SDK integration.
pub const COMMUNITY_SDK_BOOST: f64 = 0.05;

/// Integrations that satisfy the base-reward requirement: Fry dVPN and the
/// Olostep browser (registered under the id `aem`).
pub const REQUIRED_INTEGRATIONS: [&str; 2] = ["fryvpn", "aem"];

/// True for an integration that satisfies the base-reward requirement.
pub fn is_required(id: &str) -> bool {
    REQUIRED_INTEGRATIONS.contains(&id)
}

/// The base-reward component. One healthy required integration is enough for
/// the full base; the second one pays through `boost` instead.
pub fn required_component(required_active: u32) -> f64 {
    if required_active >= 1 {
        1.0
    } else {
        0.0
    }
}

/// Total boost fraction earned on top of the base component. Optional
/// integrations contribute here regardless of `required_active`.
pub fn boost(required_active: u32, partner_active: u32, community_active: u32) -> f64 {
    let second_required = if required_active >= 2 {
        SECOND_REQUIRED_BOOST
    } else {
        0.0
    };
    second_required
        + OFFICIAL_PARTNER_BOOST * partner_active as f64
        + COMMUNITY_SDK_BOOST * community_active as f64
}

/// The integration multiplier applied to the base reward, before the staking
/// multiplier and BYOD factor.
pub fn reward_multiplier(required_active: u32, partner_active: u32, community_active: u32) -> f64 {
    required_component(required_active) + boost(required_active, partner_active, community_active)
}

/// How many integrations of each reward category are active right now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActiveCounts {
    pub required: u32,
    pub partner: u32,
    pub community: u32,
}

impl ActiveCounts {
    pub fn multiplier(&self) -> f64 {
        reward_multiplier(self.required, self.partner, self.community)
    }
}

/// Count active integrations per reward category from live state.
///
/// "Active" is `enabled && healthy` — the single definition shared by the
/// estimate, the category badges and what the PoC reporter submits. An
/// integration that is enabled but unhealthy earns nothing, so counting it
/// would promise a reward the server will not pay.
///
/// The official-partner category deliberately EXCLUDES the two required
/// integrations: they pay through the base component and the second-required
/// boost, so counting them again here would pay for them twice.
pub fn count_active<'a, I>(states: I) -> ActiveCounts
where
    I: IntoIterator<Item = (&'a str, bool, bool)>,
{
    let mut counts = ActiveCounts::default();
    for (id, enabled, healthy) in states {
        if !(enabled && healthy) {
            continue;
        }
        if is_required(id) {
            counts.required += 1;
        } else if matches!(super::tier_for(id), super::IntegrationTier::Official) {
            counts.partner += 1;
        } else {
            counts.community += 1;
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ordering is a product contract: adding the second required
    /// integration must always be worth more than any single optional.
    #[test]
    fn boost_ordering_contract_holds() {
        assert!(SECOND_REQUIRED_BOOST > OFFICIAL_PARTNER_BOOST);
        assert!(OFFICIAL_PARTNER_BOOST > COMMUNITY_SDK_BOOST);
        assert!(COMMUNITY_SDK_BOOST > 0.0);
    }

    #[test]
    fn one_required_integration_earns_the_full_base() {
        assert_eq!(required_component(1), 1.0);
        assert_eq!(required_component(2), 1.0);
        assert_eq!(reward_multiplier(1, 0, 0), 1.0);
    }

    #[test]
    fn no_required_integration_earns_no_base() {
        assert_eq!(required_component(0), 0.0);
    }

    /// The headline change: optional integrations pay even with zero required.
    #[test]
    fn optionals_earn_without_any_required_integration() {
        assert!((reward_multiplier(0, 3, 0) - 0.30).abs() < f64::EPSILON);
        assert!((reward_multiplier(0, 0, 2) - 0.10).abs() < f64::EPSILON);
        assert!((reward_multiplier(0, 2, 1) - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn the_second_required_integration_is_the_largest_single_boost() {
        let one = reward_multiplier(1, 0, 0);
        let two = reward_multiplier(2, 0, 0);
        assert!((two - one - SECOND_REQUIRED_BOOST).abs() < f64::EPSILON);
        let one_partner = reward_multiplier(1, 1, 0) - one;
        let one_community = reward_multiplier(1, 0, 1) - one;
        assert!(two - one > one_partner);
        assert!(one_partner > one_community);
    }

    #[test]
    fn everything_active_sums_every_component() {
        // 3 official partners (mysterium, diiisco, space_acres) and 5
        // community integrations (storj, titan, sentinel, iagon, pawns).
        let m = reward_multiplier(2, 3, 5);
        assert!((m - 1.70).abs() < 1e-12, "expected 1.70, got {m}");
    }

    #[test]
    fn required_ids_are_fryvpn_and_olostep() {
        assert!(is_required("fryvpn"));
        assert!(is_required("aem"));
        assert!(!is_required("storj"));
        assert!(!is_required("mysterium"));
    }

    /// E7: PC-A reported "Required Active 2/2 · 100%" while the Fry dVPN card
    /// was red. Enabled-but-unhealthy must not count.
    #[test]
    fn enabled_but_unhealthy_is_not_active() {
        let counts = count_active([("fryvpn", true, false), ("aem", true, true)]);
        assert_eq!(counts.required, 1);
        assert_eq!(counts.multiplier(), 1.0);
    }

    #[test]
    fn disabled_integrations_never_count() {
        let counts = count_active([("mysterium", false, true), ("storj", false, true)]);
        assert_eq!(counts, ActiveCounts::default());
    }

    #[test]
    fn required_integrations_are_not_double_counted_as_partners() {
        // aem and fryvpn are Official tier, but they pay as required only.
        let counts = count_active([
            ("fryvpn", true, true),
            ("aem", true, true),
            ("diiisco", true, true),
        ]);
        assert_eq!(counts.required, 2);
        assert_eq!(counts.partner, 1);
        assert_eq!(counts.community, 0);
    }

    #[test]
    fn every_registered_id_lands_in_exactly_one_category() {
        let all = [
            "fryvpn", "aem", "mysterium", "diiisco", "space_acres", "storj", "titan", "sentinel",
            "iagon", "pawns",
        ];
        let counts = count_active(all.iter().map(|id| (*id, true, true)));
        assert_eq!(counts.required, 2);
        assert_eq!(counts.partner, 3, "mysterium, diiisco, space_acres");
        assert_eq!(counts.community, 5, "storj, titan, sentinel, iagon, pawns");
        assert_eq!(
            counts.required + counts.partner + counts.community,
            all.len() as u32
        );
    }

    /// A stale id from an older client (Presearch was removed in 0.4.x) must
    /// not silently become a community boost.
    #[test]
    fn a_retired_id_is_still_categorised_deterministically() {
        let counts = count_active([("presearch", true, true)]);
        assert_eq!(counts.community, 1, "unknown ids fall to the SDK tier");
    }
}
