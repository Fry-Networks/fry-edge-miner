import { describe, it, expect } from 'vitest'
import {
  splitByTier,
  tierCounts,
  sdkCounts,
  sdkActiveLine,
  type TierLike
} from './tierSplit'

const intg = (over: Partial<TierLike> & { id: string; tier: TierLike['tier'] }): TierLike => ({
  enabled: false,
  ...over
})

describe('splitByTier', () => {
  it('separates the two tiers', () => {
    const { official, sdk } = splitByTier([
      intg({ id: 'mysterium', tier: 'official' }),
      intg({ id: 'pawns', tier: 'sdk' }),
      intg({ id: 'aem', tier: 'official' })
    ])
    expect(official.map((i) => i.id)).toEqual(['mysterium', 'aem'])
    expect(sdk.map((i) => i.id)).toEqual(['pawns'])
  })

  it('preserves input order within each tier', () => {
    const { official } = splitByTier([
      intg({ id: 'fryvpn', tier: 'official' }),
      intg({ id: 'storj', tier: 'sdk' }),
      intg({ id: 'diiisco', tier: 'official' })
    ])
    expect(official.map((i) => i.id)).toEqual(['fryvpn', 'diiisco'])
  })

  it('returns empty tiers rather than throwing on an empty list', () => {
    expect(splitByTier([])).toEqual({ official: [], sdk: [] })
  })
})

describe('tierCounts', () => {
  it('counts enabled members as active', () => {
    const counts = tierCounts([
      intg({ id: 'a', tier: 'official', enabled: true }),
      intg({ id: 'b', tier: 'official', enabled: false }),
      intg({ id: 'c', tier: 'official', enabled: true })
    ])
    expect(counts.activeCount).toBe(2)
    expect(counts.availableTotal).toBe(3)
  })

  it('excludes hardware-unavailable members from the denominator', () => {
    // Same rule the category badges use — a slot the user cannot fill must
    // never be counted against them.
    const counts = tierCounts([
      intg({ id: 'a', tier: 'sdk', enabled: true }),
      intg({ id: 'iagon', tier: 'sdk', unavailable_reason: 'Iagon requires at least 900 GB' })
    ])
    expect(counts.activeCount).toBe(1)
    expect(counts.availableTotal).toBe(1)
    expect(counts.unavailableCount).toBe(1)
  })

  it('is zero across the board for an empty tier', () => {
    expect(tierCounts([])).toEqual({ activeCount: 0, availableTotal: 0, unavailableCount: 0 })
  })
})

// The headline moved to required-active counting in v0.4.18; the community
// tier keeps its own secondary line, counted over the sdk tier only.
describe('sdkCounts', () => {
  const fleet = [
    intg({ id: 'mysterium', tier: 'official', enabled: true }),
    intg({ id: 'aem', tier: 'official', enabled: true }),
    intg({ id: 'fryvpn', tier: 'official' }),
    intg({ id: 'diiisco', tier: 'official' }),
    intg({ id: 'space_acres', tier: 'official' }),
    intg({ id: 'pawns', tier: 'sdk', enabled: true }),
    intg({ id: 'storj', tier: 'sdk' }),
    intg({ id: 'titan', tier: 'sdk' }),
    intg({ id: 'sentinel', tier: 'sdk' }),
    intg({ id: 'iagon', tier: 'sdk' })
  ]

  it('counts only community builds for the secondary line', () => {
    expect(sdkCounts(fleet)).toEqual({
      activeCount: 1,
      availableTotal: 5,
      unavailableCount: 0
    })
  })

  it('still drops hardware-unavailable members from the denominator', () => {
    const withUnavailable = [
      ...fleet,
      intg({ id: 'ghost', tier: 'sdk', unavailable_reason: 'needs 900 GB' })
    ]
    const counts = sdkCounts(withUnavailable)
    expect(counts.availableTotal).toBe(5)
    expect(counts.unavailableCount).toBe(1)
  })

  it('is all zeroes for an empty list rather than throwing', () => {
    expect(sdkCounts([])).toEqual({ activeCount: 0, availableTotal: 0, unavailableCount: 0 })
  })
})

describe('sdkActiveLine', () => {
  it('reads as a bonus on top of the official count', () => {
    expect(sdkActiveLine(3)).toBe('+3 community active')
    expect(sdkActiveLine(1)).toBe('+1 community active')
  })

  it('is omitted entirely when no community integration is running', () => {
    // "+0 community active" is noise — the section already says so.
    expect(sdkActiveLine(0)).toBeNull()
    expect(sdkActiveLine(-1)).toBeNull()
  })
})
