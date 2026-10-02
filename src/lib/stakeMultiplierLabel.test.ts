import { describe, it, expect } from 'vitest'
import { deriveRewardDisplay } from './rewardReadiness'
import type { RewardSummary } from './types'

// D-C4-1 (row 11): `stakeMultiplierLabel` used `stake_multiplier.toFixed(1)`,
// which rounds a real 1.25x staking tier down to a displayed "1.3x" — the
// label no longer matched the multiplier every estimate on the page actually
// used. Pins the fixed precision rule directly: a real fraction shows its
// true value (1.25 -> "1.25x"), and a value that already displayed correctly
// under the old code stays unchanged (1.0 -> "1.0x", 3.0 -> "3.0x").
//
// Scope, per the operator's decision: nothing beyond this formatter's display
// strings is covered by this item.

const readySummary = (stake_multiplier: number): RewardSummary => ({
  active_count: 2,
  total_count: 5,
  proportion: 1,
  estimated_daily: 1,
  base_reward: 1,
  reward_amount: 1,
  reward_token_asa_id: '3612979527',
  reward_token_name: 'FRY',
  stake_token_asa_id: '',
  stake_token_name: '—',
  stake_multiplier,
  stake_label: 'Bronze',
  config_ready: true,
  stake_data_ready: true
})

describe('stakeMultiplierLabel precision', () => {
  it('shows a real fractional multiplier at its own precision, not rounded to one decimal', () => {
    expect(deriveRewardDisplay(readySummary(1.25)).stakeMultiplierLabel).toBe('1.25×')
  })

  it('a value that already displayed correctly under toFixed(1) is unchanged', () => {
    expect(deriveRewardDisplay(readySummary(1.0)).stakeMultiplierLabel).toBe('1.0×')
    expect(deriveRewardDisplay(readySummary(3.0)).stakeMultiplierLabel).toBe('3.0×')
  })

  it('a one-decimal fraction is unaffected by the two-decimal formatting', () => {
    expect(deriveRewardDisplay(readySummary(1.5)).stakeMultiplierLabel).toBe('1.5×')
  })
})
