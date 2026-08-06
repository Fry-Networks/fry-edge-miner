import { describe, it, expect } from 'vitest'
import { INTEGRATION_META } from './integrationMeta'

describe('integrationMeta', () => {
  it('should have exactly 10 integration entries', () => {
    expect(INTEGRATION_META).toHaveLength(10)
  })

  it('should have all unique ids', () => {
    const ids = INTEGRATION_META.map(m => m.id)
    const uniqueIds = new Set(ids)
    expect(uniqueIds.size).toBe(INTEGRATION_META.length)
  })

  it('should include the 4 surviving new integrations', () => {
    const newIds = ['titan', 'sentinel', 'iagon', 'pawns']
    const actualIds = INTEGRATION_META.map(m => m.id)

    newIds.forEach(id => {
      expect(actualIds).toContain(id)
    })
  })

  it('should not offer the retired filecoin_checker integration', () => {
    expect(INTEGRATION_META.map(m => m.id)).not.toContain('filecoin_checker')
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
