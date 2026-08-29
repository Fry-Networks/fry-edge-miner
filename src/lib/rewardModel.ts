// The FEM integration reward model, mirrored from the Rust client copy in
// `src-tauri/src/integrations/reward_model.rs`.
//
// One required integration active earns the FULL base reward; the second one is
// the largest single boost available. Optional integrations earn their boost
// whether or not a required integration is running, so a device with no
// required integration still earns base × boost × multipliers.
//
// These constants live in exactly three places and must change together:
//   * dbrewards `src/reward.ts` (`computeFemIntegrationTier`) — AUTHORITATIVE.
//     The server recomputes every count from the per-integration health data in
//     the PoC document and never trusts a client-supplied proportion.
//   * `src-tauri/src/integrations/reward_model.rs` — the on-device estimate.
//   * this module — what the UI renders.

import { isRequiredIntegration, type IntegrationTier } from './integrationMeta'

/** Boost for running BOTH required integrations rather than one. */
export const SECOND_REQUIRED_BOOST = 0.15
/** Boost per active official-partner integration (excludes the required two). */
export const OFFICIAL_PARTNER_BOOST = 0.1
/** Boost per active community/SDK integration. */
export const COMMUNITY_SDK_BOOST = 0.05

/** Minimal shape the reward model needs from an integration. */
export interface RewardCountable {
  id: string
  enabled: boolean
  healthy: boolean
  tier: IntegrationTier
}

/**
 * The single definition of "active" behind every count the UI shows: enabled
 * AND healthy. An enabled-but-unhealthy integration earns nothing server-side,
 * so counting it promised a reward that never arrived (E7: "Required Active
 * 2/2 · 100%" while the Fry dVPN card was red).
 */
export function isActive(i: { enabled: boolean; healthy: boolean }): boolean {
  return i.enabled && i.healthy
}

export interface ActiveCounts {
  required: number
  partner: number
  community: number
}

/**
 * Active counts per reward category. The partner category deliberately
 * EXCLUDES the two required integrations: they pay through the base component
 * and the second-required boost, so counting them again would pay them twice.
 */
export function countActive(members: RewardCountable[]): ActiveCounts {
  const active = members.filter(isActive)
  return {
    required: active.filter((i) => isRequiredIntegration(i.id)).length,
    partner: active.filter((i) => !isRequiredIntegration(i.id) && i.tier === 'official').length,
    community: active.filter((i) => !isRequiredIntegration(i.id) && i.tier !== 'official').length
  }
}

/** 1.0 once at least one required integration is active, else 0.0. */
export function requiredComponent(requiredActive: number): number {
  return requiredActive >= 1 ? 1 : 0
}

/** Total boost fraction earned on top of the base component. */
export function boostFraction(counts: ActiveCounts): number {
  return (
    (counts.required >= 2 ? SECOND_REQUIRED_BOOST : 0) +
    OFFICIAL_PARTNER_BOOST * counts.partner +
    COMMUNITY_SDK_BOOST * counts.community
  )
}

/** What the base reward is multiplied by, before staking and BYOD factors. */
export function rewardMultiplier(counts: ActiveCounts): number {
  return requiredComponent(counts.required) + boostFraction(counts)
}

/** Whole-percent boost for display, e.g. 0.25 → 25. */
export function boostDisplayPct(counts: ActiveCounts): number {
  return Math.round(boostFraction(counts) * 100)
}

/** The boost one more integration of this kind would add, for badge copy. */
export function badgeBoostPct(id: string, tier: IntegrationTier): number {
  if (isRequiredIntegration(id)) return Math.round(SECOND_REQUIRED_BOOST * 100)
  return Math.round((tier === 'official' ? OFFICIAL_PARTNER_BOOST : COMMUNITY_SDK_BOOST) * 100)
}
