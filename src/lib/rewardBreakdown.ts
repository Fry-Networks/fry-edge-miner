import { isSummaryReady } from './rewardReadiness'
import { boostDisplayPct, requiredComponent, type ActiveCounts } from './rewardModel'
import type { RewardSummary } from './types'

export interface RewardBreakdown {
  requiredPct: number
  boostPct: number
  optionalActive: number
  secondRequired: boolean
}

/**
 * B22 D2: Dashboard's "% base + % boost" label was computed from the LIVE
 * integration list (30s poll, health-event driven) while "Daily Estimate"
 * beside it came from a frozen reward summary — two different-aged
 * snapshots of the same underlying counts. Prefer the summary's own
 * breakdown (the same numbers the estimate was actually computed from);
 * fall back to the live-list computation when the summary doesn't carry one
 * (older backend, not yet ready, or browser-preview mode).
 */
export function rewardBreakdown(summary: RewardSummary | null | undefined, counts: ActiveCounts): RewardBreakdown {
  if (isSummaryReady(summary) && summary && summary.required_component !== undefined && summary.boost !== undefined) {
    return {
      requiredPct: Math.round(summary.required_component * 100),
      boostPct: Math.round(summary.boost * 100),
      optionalActive: (summary.partner_active ?? counts.partner) + (summary.community_active ?? counts.community),
      secondRequired: (summary.required_active ?? counts.required) >= 2
    }
  }
  return {
    requiredPct: Math.round(requiredComponent(counts.required) * 100),
    boostPct: boostDisplayPct(counts),
    optionalActive: counts.partner + counts.community,
    secondRequired: counts.required >= 2
  }
}
