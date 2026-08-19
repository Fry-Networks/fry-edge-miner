import { describe, it, expect } from 'vitest'
import { toFrontend } from './useIntegrations'
import type { IntegrationStatus } from '../lib/types'

// F1: the card model has to carry a tier or IntCard cannot pick a badge and
// the Dashboard cannot split its sections. The backend is authoritative, but
// a status payload from an older FEM build has no `tier` at all — that case
// must fall back to the meta table rather than render an untiered card.

const status = (over: Partial<IntegrationStatus> = {}): IntegrationStatus => ({
  id: 'mysterium',
  display_name: 'Mysterium',
  enabled: false,
  health: 'Stopped',
  lifecycle: 'Disabled',
  version: '0.1.0',
  poc_contribution: 0,
  ...over
})

describe('toFrontend tier', () => {
  it('passes the backend tier straight through', () => {
    const [out] = toFrontend([status({ id: 'pawns', tier: 'sdk' })])
    expect(out.tier).toBe('sdk')
  })

  it('trusts the backend over the meta table when they disagree', () => {
    // A partner promoted server-side must not need a client release.
    const [out] = toFrontend([status({ id: 'storj', tier: 'official' })])
    expect(out.tier).toBe('official')
  })

  it('falls back to the meta table when the backend sends no tier', () => {
    const [official] = toFrontend([status({ id: 'mysterium' })])
    expect(official.tier).toBe('official')
    const [sdk] = toFrontend([status({ id: 'titan', display_name: 'Titan Network' })])
    expect(sdk.tier).toBe('sdk')
  })

  it('never promotes an id neither side knows to official', () => {
    const [out] = toFrontend([status({ id: 'not_a_real_integration' })])
    expect(out.tier).toBe('sdk')
  })

  it('gives every integration in the catalogue a tier', () => {
    const all = toFrontend(
      ['aem', 'diiisco', 'fryvpn', 'mysterium', 'space_acres', 'iagon', 'pawns', 'sentinel', 'storj', 'titan'].map(
        (id) => status({ id })
      )
    )
    expect(all).toHaveLength(10)
    expect(all.filter((i) => i.tier === 'official')).toHaveLength(5)
    expect(all.filter((i) => i.tier === 'sdk')).toHaveLength(5)
  })
})
