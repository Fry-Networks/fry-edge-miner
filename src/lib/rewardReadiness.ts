import type { RewardSummary } from './types'

const DASH = '—'

/**
 * A summary is only "ready" once both cold-cache signals have resolved.
 * Before the first PoC-loop tick (60s interval, network round-trips first),
 * `get_reward_summary` already returns a non-null summary, but base_reward,
 * stake_multiplier, and reward_token_asa_id are placeholder defaults — not
 * real data. Every page must gate on this before showing those fields.
 */
export function isSummaryReady(summary: RewardSummary | null | undefined): boolean {
  return !!summary && summary.config_ready && summary.stake_data_ready
}

export interface RewardDisplay {
  estimated: string
  rewardToken: string
  rewardAsa: string
  baseReward: string
  stakeMultiplierLabel: string
  stakeLabel: string
}

/**
 * Pure derivation of every reward-summary display string the Dashboard,
 * Rewards, and Settings pages render. Gated on `isSummaryReady` so a
 * cold-cache summary never displays as if it were confirmed data (a false
 * 1.0x multiplier, a 0.00 base reward, or "(ASA )" with an empty id).
 */
export function deriveRewardDisplay(summary: RewardSummary | null | undefined): RewardDisplay {
  if (!isSummaryReady(summary) || !summary) {
    return {
      estimated: DASH,
      rewardToken: DASH,
      rewardAsa: DASH,
      baseReward: DASH,
      stakeMultiplierLabel: DASH,
      stakeLabel: DASH
    }
  }
  return {
    estimated: summary.estimated_daily.toFixed(2),
    rewardToken: summary.reward_token_name,
    rewardAsa: summary.reward_token_asa_id,
    baseReward: summary.base_reward.toFixed(2),
    stakeMultiplierLabel: `${summary.stake_multiplier.toFixed(1)}×`,
    stakeLabel: summary.stake_label
  }
}
