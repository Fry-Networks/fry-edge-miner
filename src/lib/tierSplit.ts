// F2: presentation-only split of the integration list into official partners
// and community SDK builds.
//
// Deliberately NOT a reward rule. `poc_contribution` and the PoC denominator
// stay exactly where they were (Rust `available_count()` / lib/availability
// `categoryCounts`) — this module only decides what the Dashboard shows in
// which section. Counting reuses categoryCounts so the two views can never
// disagree about what "active" or "available" means.

import { categoryCounts, type AvailabilityLike } from './availability'
import type { IntegrationTier } from './integrationMeta'

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

/** Secondary Dashboard line for the community tier, or null when there is none. */
export function sdkActiveLine(sdkActiveCount: number): string | null {
  if (sdkActiveCount <= 0) return null
  return `+${sdkActiveCount} community active`
}
