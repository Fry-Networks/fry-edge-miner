import { describe, expect, test } from 'vitest'
import { splitByRewardRole } from './tierSplit'

// FR: the Dashboard shows a dedicated "Required Integrations" section
// (fryvpn + aem) above Official Partners, and the partner grid no longer
// repeats the two required cards.

const member = (id: string) => ({ id, enabled: true, tier: 'official' as const })

describe('splitByRewardRole', () => {
  test('separates required members from the rest, preserving order', () => {
    const members = [
      member('mysterium'),
      member('fryvpn'),
      member('space_acres'),
      member('aem'),
      member('diiisco')
    ]
    const { required, boost } = splitByRewardRole(members)
    expect(required.map((m) => m.id)).toEqual(['fryvpn', 'aem'])
    expect(boost.map((m) => m.id)).toEqual(['mysterium', 'space_acres', 'diiisco'])
  })

  test('a list with no required members yields an empty required side', () => {
    const { required, boost } = splitByRewardRole([member('storj')])
    expect(required).toEqual([])
    expect(boost.map((m) => m.id)).toEqual(['storj'])
  })
})
