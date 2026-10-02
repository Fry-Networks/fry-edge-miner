import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, it, expect } from 'vitest'
import { deriveRewardDisplay } from './rewardReadiness'
import type { RewardSummary } from './types'

// B22 D3: Rewards' "Full Day Est. 14.88 … at full proportion" is base_reward
// with the stake multiplier omitted, sitting next to a "STAKING TIER 3.0x"
// card, with no on-screen relation to Dashboard's "DAILY ESTIMATE 55.80" —
// two differently-scoped daily figures under near-identical headline
// labels. Also settles §6's tier-vs-mult question: Rewards' "Staking Tier"
// and Dashboard's "Staking mult" are the SAME field (both
// deriveRewardDisplay(summary).stakeMultiplierLabel) — no relabelling.

function fixture(overrides: Partial<RewardSummary> = {}): RewardSummary {
  return {
    active_count: 2,
    total_count: 10,
    proportion: 1,
    estimated_daily: 14.88 * 1.25 * 3.0,
    base_reward: 14.88,
    reward_amount: 14.88,
    reward_token_asa_id: '2485202024',
    reward_token_name: 'fNODE',
    stake_token_asa_id: '',
    stake_token_name: '',
    stake_multiplier: 3.0,
    stake_label: 'No stake',
    integration_multiplier: 1.25,
    config_ready: true,
    stake_data_ready: true,
    ...overrides
  }
}

describe('deriveRewardDisplay reward figures (B22 D3)', () => {
  it('fixture A (integration_multiplier 1.25): baseReward, estimated, stakeMultiplierLabel', () => {
    const a = fixture()
    expect(deriveRewardDisplay(a).baseReward).toBe('14.88')
    expect(deriveRewardDisplay(a).estimated).toBe('55.80')
    expect(deriveRewardDisplay(a).stakeMultiplierLabel).toBe('3.0×')
  })

  it('fixture B (integration_multiplier 1.40): estimated is the other reported screenshot number', () => {
    const b = fixture({ integration_multiplier: 1.4, estimated_daily: 14.88 * 1.4 * 3.0 })
    expect(deriveRewardDisplay(b).estimated).toBe('62.50')
  })

  it('the scope gap: baseReward ("Full Day Est.") and estimated ("Daily Estimate") are different quantities', () => {
    const a = fixture()
    // The screenshot pair: 14.88 (Full Day Est.) vs 55.80 (Daily Estimate).
    expect(deriveRewardDisplay(a).baseReward).not.toBe(deriveRewardDisplay(a).estimated)
  })

  it('estimated always equals base * (required_component + boost, or integration_multiplier) * stake, across triples', () => {
    const triples: [number, number, number][] = [
      [14.88, 1.25, 3.0], // 55.80 — the Dashboard screenshot
      [14.88, 1.4, 3.0], // 62.50 — v0.4.33's "100% base + 30% boost" report
      [59.52, 1.0, 1.0],
      [59.52, 1.15, 1.5],
      [10.0, 1.0, 0.0],
      [59.52, 1.3, 3.0]
    ]
    for (const [base, integ, stake] of triples) {
      const s = fixture({ base_reward: base, integration_multiplier: integ, stake_multiplier: stake, estimated_daily: base * integ * stake })
      expect(Number(deriveRewardDisplay(s).estimated)).toBeCloseTo(base * integ * stake, 2)
    }
  })

  it('control: a cold-cache summary renders every field as the placeholder', () => {
    const cold = fixture({ config_ready: false })
    const d = deriveRewardDisplay(cold)
    expect(d.estimated).toBe('—')
    expect(d.baseReward).toBe('—')
    expect(d.stakeMultiplierLabel).toBe('—')
  })
})

describe('tier === mult: one field, two page labels (source-guarded so nobody re-introduces two sources)', () => {
  const REWARDS_SRC = readFileSync(fileURLToPath(new URL('../pages/Rewards.tsx', import.meta.url)), 'utf-8')
  const DASHBOARD_SRC = readFileSync(fileURLToPath(new URL('../pages/Dashboard.tsx', import.meta.url)), 'utf-8')

  it('both pages read stakeMultiplierLabel', () => {
    expect(REWARDS_SRC).toContain('stakeMultiplierLabel')
    expect(DASHBOARD_SRC).toContain('stakeMultiplierLabel')
  })

  it('Rewards.tsx does not compute a second stake_multiplier expression of its own', () => {
    // Property-access form only — a prose comment mentioning the field name
    // (e.g. "base_reward/stake_multiplier") is not a second source.
    const hits = REWARDS_SRC.match(/\.stake_multiplier\b/g) ?? []
    expect(hits.length).toBe(0) // only reads the already-derived stakeMultiplierLabel
  })

  it('Rewards.tsx no longer sub-labels Full Day Est. as if it already included the stake multiplier', () => {
    expect(REWARDS_SRC).not.toContain('sub={`${rewardToken} at full proportion`}')
  })

  it('Rewards.tsx now reconciles the two figures with a sub2 line', () => {
    expect(REWARDS_SRC).toMatch(/sub2=/)
  })

  it('Full Day Est. label and value stay byte-identical (rewards.spec.ts / empty-states.spec.ts pin these)', () => {
    expect(REWARDS_SRC).toContain('label="Full Day Est."')
    expect(REWARDS_SRC).toContain('value={fullDayEst}')
  })
})
