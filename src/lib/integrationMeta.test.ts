import { describe, it, expect } from 'vitest'
import { INTEGRATION_META } from './integrationMeta'

describe('integrationMeta', () => {
  it('should have exactly 11 integration entries', () => {
    expect(INTEGRATION_META).toHaveLength(11)
  })

  it('should have all unique ids', () => {
    const ids = INTEGRATION_META.map(m => m.id)
    const uniqueIds = new Set(ids)
    expect(uniqueIds.size).toBe(INTEGRATION_META.length)
  })

  it('should include the 5 new integrations', () => {
    const newIds = ['titan', 'filecoin_checker', 'sentinel', 'iagon', 'pawns']
    const actualIds = INTEGRATION_META.map(m => m.id)

    newIds.forEach(id => {
      expect(actualIds).toContain(id)
    })
  })

  it('should have all required properties for each entry', () => {
    INTEGRATION_META.forEach(entry => {
      expect(entry).toHaveProperty('id')
      expect(entry).toHaveProperty('name')
      expect(entry).toHaveProperty('tag')
      expect(entry).toHaveProperty('desc')
      expect(entry).toHaveProperty('Icon')
      expect(entry).toHaveProperty('col')
      expect(entry).toHaveProperty('uptime')
    })
  })
})
