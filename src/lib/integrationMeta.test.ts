import { describe, it, expect } from 'vitest'
import { CATEGORIES, INTEGRATION_META } from './integrationMeta'

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

  it('assigns every integration to a known category', () => {
    for (const m of INTEGRATION_META) {
      expect(CATEGORIES).toContain(m.category)
    }
  })

  it('groups integrations into the three expected categories', () => {
    const byCategory = (cat: string) =>
      INTEGRATION_META.filter(m => m.category === cat).map(m => m.id).sort()

    expect(byCategory('VPN & Bandwidth')).toEqual(['fryvpn', 'mysterium', 'pawns', 'sentinel'])
    expect(byCategory('Storage & Farming')).toEqual(['iagon', 'space_acres', 'storj', 'titan'])
    expect(byCategory('AI & Data')).toEqual(['aem', 'diiisco'])
  })

  it('shows the aem integration as "Olostep" — never as AEM', () => {
    const aem = INTEGRATION_META.find(m => m.id === 'aem')
    expect(aem?.name).toBe('Olostep')
    for (const m of INTEGRATION_META) {
      expect(`${m.name} ${m.tag} ${m.desc}`).not.toMatch(/\bAEM\b/)
    }
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
