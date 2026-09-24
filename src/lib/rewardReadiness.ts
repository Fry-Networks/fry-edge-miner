import type { RewardSummary } from './types'

const DASH = '—'

/**
 * 1.25 -> "1.25", 1.0 -> "1.0", 3.0 -> "3.0", 1.5 -> "1.5". `toFixed(1)`
 * rounded a real 1.25x staking tier down to a displayed "1.3x" — the label no
 * longer matched the multiplier every estimate on the page actually used.
 * Format to 2 decimals (the same precision `estimated`/`baseReward` already
 * use below) and drop a trailing zero in the hundredths place, but never
 * below one decimal, so a whole-number tier still reads "1.0x"/"3.0x" rather
 * than "1x"/"3x".
 */
function formatMultiplier(value: number): string {
  const twoDp = value.toFixed(2)
  return twoDp.endsWith('0') ? twoDp.slice(0, -1) : twoDp
}

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
    stakeMultiplierLabel: `${formatMultiplier(summary.stake_multiplier)}×`,
    stakeLabel: summary.stake_label
  }
}

/**
 * B22 D1: "No stake" + " stake active" reads "No stake stake active". Same
 * de-dup guard SettingsPage.tsx's stake line already applies, extracted here
 * so Rewards.tsx can use it too.
 */
export function stakeSubtitle(stakeLabel: string): string {
  if (stakeLabel === DASH) return DASH
  return `${stakeLabel}${stakeLabel.toLowerCase().includes('stake') ? '' : ' stake'} active`
}
