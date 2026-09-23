import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, it, expect } from 'vitest'
import { integrationBadge } from '../lib/integrationBadge'

// G4 review finding 15 (B14 Done-when: "card badge and tile derive from one
// state source ... for every integration across states"): Dashboard's
// MiniCard called integrationBadge(intg) with a bare DashboardIntegration
// that has no `dockerBlocked` field, so a docker-requiring, uninstalled,
// disabled integration read "Unavailable" on the Integrations card but
// "Not installed" on the Dashboard tile — the exact class of desync B14
// exists to remove, just on the one input (dockerBlocked) the original fix
// didn't thread through to the tile.

describe('integrationBadge agrees regardless of which page supplies dockerBlocked (regression proof)', () => {
  it('is Unavailable, not Not installed, when docker-blocked — same input either page could supply', () => {
    const base = {
      enabled: false,
      healthy: false,
      health: 'Stopped' as const,
      lifecycle: 'Disabled' as const,
      version: null,
      unavailable_reason: null
    }
    expect(integrationBadge({ ...base, dockerBlocked: true }).label).toBe('Unavailable')
    expect(integrationBadge({ ...base, dockerBlocked: false }).label).toBe('Not installed')
  })
})

describe('Dashboard.tsx computes and threads dockerBlocked through to the tile (source guard)', () => {
  const SOURCE = readFileSync(fileURLToPath(new URL('./Dashboard.tsx', import.meta.url)), 'utf-8')

  it('DashboardIntegration carries requires_docker (the field MiniCard needs to compute dockerBlocked)', () => {
    expect(SOURCE).toMatch(/requires_docker:\s*boolean/)
  })

  it('Dashboard accepts a system prop so it can know whether docker is ready', () => {
    expect(SOURCE).toMatch(/system\??:\s*SystemStatus/)
  })

  it('MiniCard computes dockerBlocked and passes it into integrationBadge', () => {
    expect(SOURCE).toMatch(/dockerBlocked\s*=\s*intg\.requires_docker\s*&&\s*dockerNotReady/)
    expect(SOURCE).toMatch(/integrationBadge\(\{\s*\.\.\.intg,\s*dockerBlocked\s*\}\)/)
  })

  it('every MiniCard call site passes dockerNotReady down', () => {
    const calls = SOURCE.match(/<MiniCard\b[^>]*\/>/g) ?? []
    expect(calls.length).toBeGreaterThanOrEqual(3)
    for (const call of calls) {
      expect(call, `MiniCard call site missing dockerNotReady: ${call}`).toMatch(/dockerNotReady/)
    }
  })
})
