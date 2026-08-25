// F2: presentation-only split of the integration list into official partners
// and community SDK builds.
//
// Deliberately NOT a reward rule. `poc_contribution` and the PoC denominator
// stay exactly where they were (Rust `available_count()` / lib/availability
// `categoryCounts`) — this module only decides what the Dashboard shows in
// which section. Counting reuses categoryCounts so the two views can never
// disagree about what "active" or "available" means.

import { categoryCounts, type AvailabilityLike } from './availability'
import { isRequiredIntegration, type IntegrationTier } from './integrationMeta'

export interface TierLike extends AvailabilityLike {
  tier: IntegrationTier
}

export interface TierSplit<T> {
  official: T[]
  sdk: T[]
}

/** Partition a list into its two tiers, preserving input order within each. */
export function splitByTier<T extends TierLike>(members: T[]): TierSplit<T> {
  return {
    official: members.filter((i) => i.tier === 'official'),
    sdk: members.filter((i) => i.tier === 'sdk')
  }
}

export interface RewardRoleSplit<T> {
  required: T[]
  boost: T[]
}

/**
 * Partition by reward role: the REQUIRED integrations (Fry dVPN + Olostep)
 * against everything else, preserving input order. Presentation-only, like
 * splitByTier — it feeds the dedicated "Required Integrations" section and
 * keeps the partner grid from repeating those two cards.
 */
export function splitByRewardRole<T extends { id: string }>(members: T[]): RewardRoleSplit<T> {
  return {
    required: members.filter((i) => isRequiredIntegration(i.id)),
    boost: members.filter((i) => !isRequiredIntegration(i.id))
  }
}

/**
 * Counts for one tier, using the same active/available semantics as the
 * category badges: unavailable members are excluded from the denominator so
 * the headline never implies a slot the user could fill.
 */
export function tierCounts(members: TierLike[]): {
  activeCount: number
  availableTotal: number
  unavailableCount: number
} {
  return categoryCounts(members)
}

/**
 * Counts for the official tier of a full integration list.
 *
 * No longer the headline figure: the Dashboard StatCard and the sidebar badge
 * count REQUIRED integrations via `requiredActiveCount` now that the reward
 * model is required-driven. Kept for tier-level counting.
 */
export function officialCounts(members: TierLike[]) {
  return tierCounts(splitByTier(members).official)
}

/** Counts for the community tier of a full integration list. */
export function sdkCounts(members: TierLike[]) {
  return tierCounts(splitByTier(members).sdk)
}

/** Secondary line for the community tier, or null when there is none. */
export function sdkActiveLine(sdkActiveCount: number): string | null {
  if (sdkActiveCount <= 0) return null
  return `+${sdkActiveCount} community active`
}
