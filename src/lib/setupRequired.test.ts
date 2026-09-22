import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, it, expect } from 'vitest'
import { awaitsUserSetup } from './types'
import { hasFailingIntegration } from './setupRequired'
import { integrationBadge } from './integrationBadge'
import type { HealthStatus } from './types'

// B17 D2: awaitsUserSetup() was a startsWith('Awaiting Storj setup') prefix
// test, so Iagon's "node token not provisioned" and Sentinel's "node account
// not funded" reasons fell through to the red Unhealthy badge instead of the
// amber "Setup required" one — and the backend supervisor restart-loops
// both partners every 30s for the same reason (B17 D1, T3-owned).

const IAGON = 'Iagon node token not provisioned — register a node at app.iagon.com and save the key as {"node_token": "<key>"} in /tmp/config.json'
const SENTINEL = 'Sentinel node account not funded — send DVPN to sent1qqqqexample to activate this node'
const STORJ = 'Awaiting Storj setup — create a node auth token at storj.io and complete node identity to bring this node online. Install and eligibility are already active.'
const PAWNS = 'needs your consent'

describe('awaitsUserSetup (B17 D2)', () => {
  it('recognises the Iagon missing-token reason', () => {
    expect(awaitsUserSetup({ Unhealthy: IAGON })).toBe(true)
  })

  it('recognises the Sentinel unfunded-account reason', () => {
    expect(awaitsUserSetup({ Unhealthy: SENTINEL })).toBe(true)
  })

  it('still recognises Storj and Pawns (additivity)', () => {
    expect(awaitsUserSetup({ Unhealthy: STORJ })).toBe(true)
    expect(awaitsUserSetup({ Unhealthy: PAWNS })).toBe(true)
  })

  it('leaves a genuine partner crash classified as a failure', () => {
    expect(awaitsUserSetup({ Unhealthy: 'storagenode exited with code 1' })).toBe(false)
    expect(awaitsUserSetup({ Unhealthy: 'Container exited: panic' })).toBe(false)
    expect(awaitsUserSetup({ Unhealthy: 'docker compose ps failed: exit 1' })).toBe(false)
  })

  it('is false for every non-Unhealthy status', () => {
    expect(awaitsUserSetup('Healthy')).toBe(false)
    expect(awaitsUserSetup('Stopped')).toBe(false)
    expect(awaitsUserSetup('Starting')).toBe(false)
  })
})

// B17 D3: the Dashboard tile had no setup-required state at all (no `health`
// field on DashboardIntegration), so even Storj read "Unhealthy" on the tile
// while its card read "SETUP REQUIRED". B14 D2 (landed first per the lead's
// D-12 ordering) already gave both pages one shared ladder — see
// integrationBadge.ts — so this defect is satisfied as a consequence: both
// pages call integrationBadge(), whose 'Setup required' branch calls
// awaitsUserSetup() from types.ts, which (this file, D2 above) now
// recognises all four markers. Guard that single source instead of a
// per-file literal, which no longer exists in either consumer.
describe('setup-required reaches both the card and the tile via one source (B17 D3)', () => {
  const BADGE_SRC = readFileSync(fileURLToPath(new URL('./integrationBadge.ts', import.meta.url)), 'utf-8')
  const DASHBOARD_SRC = readFileSync(fileURLToPath(new URL('../pages/Dashboard.tsx', import.meta.url)), 'utf-8')
  const INTCARD_SRC = readFileSync(fileURLToPath(new URL('../components/IntCard.tsx', import.meta.url)), 'utf-8')

  it('the shared badge ladder renders the literal label "Setup required"', () => {
    expect(BADGE_SRC).toContain('Setup required')
  })

  it('both the card and the tile consume the shared badge (already proven by badgeSingleSource.test.ts)', () => {
    expect(DASHBOARD_SRC).toContain("from '../lib/integrationBadge'")
    expect(INTCARD_SRC).toContain("from '../lib/integrationBadge'")
  })

  it('Storj/Iagon/Sentinel setup-wait reasons resolve to the same badge regardless of which page asks', () => {
    for (const reason of [
      STORJ,
      IAGON,
      SENTINEL
    ]) {
      const badge = integrationBadge({
        enabled: true,
        healthy: false,
        health: { Unhealthy: reason },
        lifecycle: 'Unhealthy',
        version: '1.0.0',
        unavailable_reason: null
      })
      expect(badge.label).toBe('Setup required')
    }
  })
})

// B17 D4: a setup-blocked integration counted as a fleet failure and painted
// the sidebar amber (App.tsx's hasUnhealthy = integrations.some((i) =>
// i.enabled && !i.healthy), with zero unit coverage).
describe('hasFailingIntegration (B17 D4)', () => {
  const health = (h: HealthStatus) => h

  it('does not flag an enabled+unhealthy entry that is only awaiting user setup', () => {
    const list = [
      { enabled: true, healthy: false, health: health({ Unhealthy: IAGON }) },
      { enabled: true, healthy: true, health: health('Healthy') }
    ]
    expect(hasFailingIntegration(list)).toBe(false)
  })

  it('still flags a genuine crash', () => {
    const list = [
      { enabled: true, healthy: false, health: health({ Unhealthy: IAGON }) },
      { enabled: true, healthy: false, health: health({ Unhealthy: 'titan-edge process is not running' }) }
    ]
    expect(hasFailingIntegration(list)).toBe(true)
  })

  it('does not flag a disabled, unhealthy entry', () => {
    const list = [{ enabled: false, healthy: false, health: health({ Unhealthy: 'titan-edge process is not running' }) }]
    expect(hasFailingIntegration(list)).toBe(false)
  })
})
