import { describe, it, expect } from 'vitest'
import { isSummaryReady, deriveRewardDisplay } from './rewardReadiness'
import type { RewardSummary } from './types'

// Reproduces the reported bug: navigating straight to the Dashboard right
// after launch fetches a summary BEFORE the 60s PoC-loop tick has warmed
// base_reward/stake data. The backend still returns a non-null summary — its
// cold-cache defaults (base_reward 0, stake_multiplier 1.0, an empty ASA id)
// used to be rendered as if they were real numbers ("0.00", "1.0×",
// "(ASA )"). config_ready/stake_data_ready let the frontend tell the two
// states apart.

const coldSummary = (over: Partial<RewardSummary> = {}): RewardSummary => ({
  active_count: 2,
  total_count: 5,
  proportion: 1,
  estimated_daily: 0,
  base_reward: 0,
  reward_amount: 0,
  reward_token_asa_id: '',
  reward_token_name: '—',
  stake_token_asa_id: '',
  stake_token_name: '—',
  stake_multiplier: 1.0,
  stake_label: 'No stake',
  config_ready: false,
  stake_data_ready: false,
  ...over
})

const warmSummary = (over: Partial<RewardSummary> = {}): RewardSummary =>
  coldSummary({
    estimated_daily: 12.5,
    base_reward: 5,
    reward_amount: 5,
    reward_token_asa_id: '3612979527',
    reward_token_name: 'FRY',
    stake_multiplier: 1.25,
    stake_label: 'Bronze',
    config_ready: true,
    stake_data_ready: true,
    ...over
  })

describe('isSummaryReady', () => {
  it('is false for null', () => {
    expect(isSummaryReady(null)).toBe(false)
  })

  it('is false while cold (neither flag resolved)', () => {
    expect(isSummaryReady(coldSummary())).toBe(false)
  })

  it('is false when only config resolved', () => {
    expect(isSummaryReady(coldSummary({ config_ready: true }))).toBe(false)
  })

  it('is false when only stake data resolved', () => {
    expect(isSummaryReady(coldSummary({ stake_data_ready: true }))).toBe(false)
  })

  it('is true once both flags resolve', () => {
    expect(isSummaryReady(warmSummary())).toBe(true)
  })
})

describe('deriveRewardDisplay', () => {
  it('never shows a cold-cache 0.00/1.0x/empty-ASA as if it were real data', () => {
    const out = deriveRewardDisplay(coldSummary())
    expect(out.estimated).toBe('—')
    expect(out.baseReward).toBe('—')
    expect(out.stakeMultiplierLabel).toBe('—')
    expect(out.rewardAsa).toBe('—')
    expect(out.rewardToken).toBe('—')
    expect(out.stakeLabel).toBe('—')
  })

  it('shows placeholders for a null summary', () => {
    const out = deriveRewardDisplay(null)
    expect(out.estimated).toBe('—')
    expect(out.baseReward).toBe('—')
  })

  it('shows the real numbers once both readiness flags are true', () => {
    const out = deriveRewardDisplay(warmSummary())
    expect(out.estimated).toBe('12.50')
    expect(out.baseReward).toBe('5.00')
    expect(out.stakeMultiplierLabel).toBe('1.3×')
    expect(out.rewardAsa).toBe('3612979527')
    expect(out.rewardToken).toBe('FRY')
    expect(out.stakeLabel).toBe('Bronze')
  })

  it('still hides stake data when only config is ready (e.g. registered device awaiting verified-stake fetch)', () => {
    const out = deriveRewardDisplay(coldSummary({ config_ready: true, base_reward: 5, estimated_daily: 5 }))
    expect(out.stakeMultiplierLabel).toBe('—')
    expect(out.estimated).toBe('—')
  })
})
