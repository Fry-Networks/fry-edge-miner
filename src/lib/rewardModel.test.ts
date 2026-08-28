import { describe, it, expect } from 'vitest'
import {
  SECOND_REQUIRED_BOOST,
  OFFICIAL_PARTNER_BOOST,
  COMMUNITY_SDK_BOOST,
  badgeBoostPct,
  boostDisplayPct,
  boostFraction,
  countActive,
  isActive,
  requiredComponent,
  rewardMultiplier,
  type RewardCountable
} from './rewardModel'

const mk = (
  id: string,
  tier: 'official' | 'sdk',
  enabled: boolean,
  healthy: boolean
): RewardCountable => ({ id, tier, enabled, healthy })

/** The full v0.4.x registry, everything running. */
const ALL_ACTIVE: RewardCountable[] = [
  mk('fryvpn', 'official', true, true),
  mk('aem', 'official', true, true),
  mk('mysterium', 'official', true, true),
  mk('diiisco', 'official', true, true),
  mk('space_acres', 'official', true, true),
  mk('storj', 'sdk', true, true),
  mk('titan', 'sdk', true, true),
  mk('sentinel', 'sdk', true, true),
  mk('iagon', 'sdk', true, true),
  mk('pawns', 'sdk', true, true)
]

describe('reward model constants', () => {
  it('keeps the ordering contract: second required > official partner > community', () => {
    expect(SECOND_REQUIRED_BOOST).toBeGreaterThan(OFFICIAL_PARTNER_BOOST)
    expect(OFFICIAL_PARTNER_BOOST).toBeGreaterThan(COMMUNITY_SDK_BOOST)
    expect(COMMUNITY_SDK_BOOST).toBeGreaterThan(0)
  })
})

describe('isActive', () => {
  it('requires both enabled and healthy', () => {
    expect(isActive({ enabled: true, healthy: true })).toBe(true)
    expect(isActive({ enabled: true, healthy: false })).toBe(false)
    expect(isActive({ enabled: false, healthy: true })).toBe(false)
  })
})

describe('countActive', () => {
  it('partitions every registered integration into exactly one category', () => {
    const c = countActive(ALL_ACTIVE)
    expect(c).toEqual({ required: 2, partner: 3, community: 5 })
    expect(c.required + c.partner + c.community).toBe(ALL_ACTIVE.length)
  })

  it('does not count a required integration as an official partner', () => {
    const c = countActive([
      mk('fryvpn', 'official', true, true),
      mk('aem', 'official', true, true)
    ])
    expect(c).toEqual({ required: 2, partner: 0, community: 0 })
  })

  // E7: PC-A showed "Required Active 2/2 · Required proportion 100%" while the
  // Fry dVPN card was red and unhealthy.
  it('excludes an enabled-but-unhealthy integration', () => {
    const c = countActive([
      mk('fryvpn', 'official', true, false),
      mk('aem', 'official', true, true)
    ])
    expect(c.required).toBe(1)
  })

  it('ignores disabled integrations even when their last health was good', () => {
    expect(countActive([mk('storj', 'sdk', false, true)])).toEqual({
      required: 0,
      partner: 0,
      community: 0
    })
  })
})

describe('reward multiplier', () => {
  it('pays the full base for a single required integration', () => {
    expect(requiredComponent(1)).toBe(1)
    expect(rewardMultiplier({ required: 1, partner: 0, community: 0 })).toBe(1)
  })

  it('makes the second required integration the largest single boost', () => {
    const one = rewardMultiplier({ required: 1, partner: 0, community: 0 })
    const two = rewardMultiplier({ required: 2, partner: 0, community: 0 })
    const partner = rewardMultiplier({ required: 1, partner: 1, community: 0 }) - one
    const community = rewardMultiplier({ required: 1, partner: 0, community: 1 }) - one
    expect(two - one).toBeCloseTo(SECOND_REQUIRED_BOOST, 10)
    expect(two - one).toBeGreaterThan(partner)
    expect(partner).toBeGreaterThan(community)
  })

  // The headline behaviour change: optionals no longer need a required
  // integration to earn anything.
  it('earns boost with zero required integrations active', () => {
    expect(requiredComponent(0)).toBe(0)
    expect(rewardMultiplier({ required: 0, partner: 3, community: 0 })).toBeCloseTo(0.3, 10)
    expect(rewardMultiplier({ required: 0, partner: 0, community: 5 })).toBeCloseTo(0.25, 10)
    expect(rewardMultiplier({ required: 0, partner: 2, community: 1 })).toBeCloseTo(0.25, 10)
  })

  it('sums every component when the whole registry is active', () => {
    expect(rewardMultiplier(countActive(ALL_ACTIVE))).toBeCloseTo(1.7, 10)
    expect(boostFraction(countActive(ALL_ACTIVE))).toBeCloseTo(0.7, 10)
    expect(boostDisplayPct(countActive(ALL_ACTIVE))).toBe(70)
  })

  it('matches the Rust client model for the documented scenarios', () => {
    expect(rewardMultiplier({ required: 0, partner: 0, community: 0 })).toBe(0)
    expect(rewardMultiplier({ required: 2, partner: 0, community: 0 })).toBeCloseTo(1.15, 10)
    expect(rewardMultiplier({ required: 1, partner: 3, community: 0 })).toBeCloseTo(1.3, 10)
    expect(rewardMultiplier({ required: 2, partner: 3, community: 5 })).toBeCloseTo(1.7, 10)
  })
})

describe('badgeBoostPct', () => {
  it('labels each category with the boost it actually earns', () => {
    expect(badgeBoostPct('fryvpn', 'official')).toBe(15)
    expect(badgeBoostPct('mysterium', 'official')).toBe(10)
    expect(badgeBoostPct('storj', 'sdk')).toBe(5)
  })
})
