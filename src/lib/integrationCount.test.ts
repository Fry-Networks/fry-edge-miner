import { describe, it, expect } from 'vitest'
import { activeFraction, proportionPct } from './integrationCount'

// Regression for the hardcoded "/5" integration-count bug (E2E QA v0.2.29):
// the UI must derive the total from the real integration list (6), never a
// hardcoded 5, so it stays correct when integrations are added/removed.
describe('integrationCount', () => {
  it('formats active/total using the real total, not a hardcoded 5', () => {
    expect(activeFraction(0, 6)).toBe('0/6')
    expect(activeFraction(1, 6)).toBe('1/6')
    // pre-fix hardcoded total showed "6/5"; must be "6/6"
    expect(activeFraction(6, 6)).toBe('6/6')
  })

  it('computes proportion percent from the real total', () => {
    expect(proportionPct(0, 6)).toBe(0)
    expect(proportionPct(1, 6)).toBe(17)
    // pre-fix "/5" gave 120% for a full house; must be 100
    expect(proportionPct(6, 6)).toBe(100)
  })

  it('guards divide-by-zero while the list is still loading', () => {
    expect(proportionPct(0, 0)).toBe(0)
    expect(activeFraction(0, 0)).toBe('0/0')
  })
})
