import { describe, it, expect } from 'vitest'
import { INTEGRATION_META } from './integrationMeta'

// F1: the tier split drives the whole Dashboard/Integrations layout and the
// "X / 5" reward copy, so the official set is asserted literally rather than
// derived — adding a sixth official partner must fail here and be a decision,
// not a side effect. Mirrors integrations/mod.rs tier_tests on the Rust side.

const OFFICIAL = ['aem', 'diiisco', 'fryvpn', 'mysterium', 'space_acres']
const SDK = ['iagon', 'pawns', 'sentinel', 'storj', 'titan']

const idsWithTier = (tier: 'official' | 'sdk') =>
  INTEGRATION_META.filter((m) => m.tier === tier)
    .map((m) => m.id)
    .sort()

describe('integration tier', () => {
  it('gives every integration a tier', () => {
    for (const m of INTEGRATION_META) {
      expect(['official', 'sdk']).toContain(m.tier)
    }
  })

  it('marks exactly the five contracted partners as official', () => {
    expect(idsWithTier('official')).toEqual(OFFICIAL)
  })

  it('marks exactly the five community SDK builds as sdk', () => {
    expect(idsWithTier('sdk')).toEqual(SDK)
  })

  it('splits the whole catalogue with no integration in both tiers', () => {
    expect(OFFICIAL.length + SDK.length).toBe(INTEGRATION_META.length)
    const overlap = OFFICIAL.filter((id) => SDK.includes(id))
    expect(overlap).toEqual([])
  })
})
