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

/**
 * Counts for the official tier of a full integration list.
 *
 * The single source for the headline figure. The Dashboard StatCard and the
 * sidebar badge both read it, which is the point: they used to derive their
 * counts independently and disagreed (sidebar `N/10` against Dashboard `N/5`).
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
