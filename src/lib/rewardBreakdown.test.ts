import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, it, expect } from 'vitest'
import { rewardBreakdown } from './rewardBreakdown'
import { deriveRewardDisplay } from './rewardReadiness'
import type { RewardSummary } from './types'

// B22 D2: Dashboard's "% base + % boost" label was computed from the LIVE
// integration list while "Daily Estimate" beside it came from a frozen
// reward summary — two different health snapshots. The reported case:
// Dashboard "Staking mult 3.0x / DAILY ESTIMATE 55.80" beside a boost label
// that had already dropped to reflect a since-changed live integration set.

function summaryFixture(overrides: Partial<RewardSummary> = {}): RewardSummary {
  return {
    active_count: 2,
    total_count: 10,
    proportion: 1,
    estimated_daily: 14.88 * 1.4 * 3.0,
    base_reward: 14.88,
    reward_amount: 14.88,
    reward_token_asa_id: '2485202024',
    reward_token_name: 'fNODE',
    stake_token_asa_id: '',
    stake_token_name: '',
    stake_multiplier: 3.0,
    stake_label: 'Gold',
    required_active: 1,
    partner_active: 1,
    community_active: 3,
    required_component: 1,
    boost: 0.4,
    integration_multiplier: 1.4,
    config_ready: true,
    stake_data_ready: true,
    ...overrides
  }
}

describe('rewardBreakdown', () => {
  it('follows the summary the estimate was computed from, not a staler live count (the 0.4.33 screenshot case)', () => {
    const s = summaryFixture()
    const staleCounts = { required: 1, partner: 1, community: 1 } // boostFraction 0.15 -> 15%
    const b = rewardBreakdown(s, staleCounts)
    expect(b.boostPct).toBe(40)
    expect(b.boostPct).not.toBe(15)
  })

  it('estimate/label coherence: 14.88 base * 1.40 integ * 3.0 stake = 62.50', () => {
    const s = summaryFixture()
    expect(deriveRewardDisplay(s).estimated).toBe('62.50')
    expect(s.base_reward * (s.required_component! + s.boost!) * s.stake_multiplier).toBeCloseTo(
      Number(deriveRewardDisplay(s).estimated),
      2
    )
  })

  it('the 55.80 case (integration_multiplier 1.25)', () => {
    const s = summaryFixture({ integration_multiplier: 1.25, boost: 0.25, estimated_daily: 14.88 * 1.25 * 3.0 })
    expect(deriveRewardDisplay(s).estimated).toBe('55.80')
    const b = rewardBreakdown(s, { required: 1, partner: 1, community: 1 })
    expect(b.boostPct).toBe(25)
    expect(b.requiredPct).toBe(100)
  })

  it('falls back to the live-list numbers when the summary has no breakdown (older backend)', () => {
    const s = summaryFixture({ boost: undefined, required_component: undefined })
    const counts = { required: 1, partner: 2, community: 1 }
    const b = rewardBreakdown(s, counts)
    // live boostFraction: OFFICIAL_PARTNER_BOOST*2 + COMMUNITY_SDK_BOOST*1 (see rewardModel.ts)
    expect(b.optionalActive).toBe(3)
    expect(b.requiredPct).toBe(100)
  })

  it('falls back to the live-list numbers when the summary is not ready', () => {
    const s = summaryFixture({ config_ready: false })
    const counts = { required: 0, partner: 1, community: 0 }
    const b = rewardBreakdown(s, counts)
    expect(b.requiredPct).toBe(0)
    expect(b.optionalActive).toBe(1)
  })

  it('secondRequired reflects the summary required_active, not a stale live count', () => {
    const s = summaryFixture({ required_active: 2 })
    const b = rewardBreakdown(s, { required: 1, partner: 0, community: 0 })
    expect(b.secondRequired).toBe(true)
  })
})

describe('Dashboard.tsx no longer computes the boost label from the live list alone', () => {
  const SOURCE = readFileSync(fileURLToPath(new URL('../pages/Dashboard.tsx', import.meta.url)), 'utf-8')

  it('does not call boostDisplayPct(counts) directly', () => {
    expect(SOURCE).not.toContain('boostDisplayPct(counts)')
  })

  it('imports rewardBreakdown', () => {
    expect(SOURCE).toContain("from '../lib/rewardBreakdown'")
  })
})
