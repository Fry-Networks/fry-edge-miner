import { describe, expect, test } from 'vitest'
import { requiredActiveCount } from './integrationCount'

// FR: the Active Integrations stat and the sidebar summary count REQUIRED
// integrations (fryvpn + aem) out of 2, not official partners out of 5 —
// two enabled required integrations already mean full base reward.

const intg = (id: string, enabled: boolean) => ({ id, enabled })

describe('requiredActiveCount', () => {
  test('counts only enabled required integrations', () => {
    const members = [
      intg('fryvpn', true),
      intg('aem', false),
      intg('mysterium', true),
      intg('space_acres', true)
    ]
    expect(requiredActiveCount(members)).toBe(1)
  })

  test('both required enabled is 2 regardless of boost activity', () => {
    const members = [intg('fryvpn', true), intg('aem', true), intg('diiisco', true)]
    expect(requiredActiveCount(members)).toBe(2)
  })

  test('nothing enabled is 0', () => {
    expect(requiredActiveCount([intg('fryvpn', false), intg('aem', false)])).toBe(0)
  })
})
