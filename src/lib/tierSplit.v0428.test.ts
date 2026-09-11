import { describe, it, expect } from 'vitest'
import { sdkActiveCount, sdkActiveLine, type TierHealthLike } from './tierSplit'

const intgHealth = (
  over: Partial<TierHealthLike> & { id: string; tier: TierHealthLike['tier'] }
): TierHealthLike => ({
  enabled: false,
  healthy: false,
  ...over
})

// BUG 13 (Discord, georgeparis 8/30): for 1 official + 1 community
// integration ACTIVE, the Sidebar read "+2 community active" while the
// Dashboard read "+1" for the identical fleet state. Root cause: the Sidebar
// fed `sdkCounts(...).activeCount` (enabled-only — correct for its OWN badge
// use) into a line whose label promises reward-sense "active" (enabled AND
// healthy). `sdkActiveCount` is the one function both surfaces must use.
describe('sdkActiveCount', () => {
  it('counts a community integration only when it is enabled AND healthy', () => {
    const fleet = [
      intgHealth({ id: 'mysterium', tier: 'official', enabled: true, healthy: true }),
      // Enabled but NOT healthy — this is exactly what inflated the old
      // enabled-only count to "+2" while only one community integration was
      // actually contributing.
      intgHealth({ id: 'storj', tier: 'sdk', enabled: true, healthy: false }),
      intgHealth({ id: 'titan', tier: 'sdk', enabled: true, healthy: true })
    ]
    expect(sdkActiveCount(fleet)).toBe(1)
  })

  it('matches the Dashboard badge count for the same fleet state', () => {
    // Same "1 official + 1 community active" state from the Discord report.
    const fleet = [
      intgHealth({ id: 'mysterium', tier: 'official', enabled: true, healthy: true }),
      intgHealth({ id: 'storj', tier: 'sdk', enabled: true, healthy: false }),
      intgHealth({ id: 'titan', tier: 'sdk', enabled: true, healthy: true })
    ]
    expect(sdkActiveLine(sdkActiveCount(fleet))).toBe('+1 community active')
  })

  it('is zero when every community integration is unhealthy', () => {
    const fleet = [intgHealth({ id: 'storj', tier: 'sdk', enabled: true, healthy: false })]
    expect(sdkActiveCount(fleet)).toBe(0)
  })

  it('ignores official-tier members even when they are active', () => {
    const fleet = [intgHealth({ id: 'mysterium', tier: 'official', enabled: true, healthy: true })]
    expect(sdkActiveCount(fleet)).toBe(0)
  })
})
