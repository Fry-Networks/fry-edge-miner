import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, it, expect } from 'vitest'

// B14 D2: card and tile had no shared state source — MiniCard derived its
// label from {enabled, healthy} only (3 of the card's 8 labels) and
// DashboardIntegration didn't even declare health/lifecycle/version. Source
// guards in the IntCardWarnings.test.ts idiom, since the vitest environment
// here is `node` and the project ships no jsdom/testing-library.

const DASHBOARD_SRC = readFileSync(fileURLToPath(new URL('../pages/Dashboard.tsx', import.meta.url)), 'utf-8')
const INTCARD_SRC = readFileSync(fileURLToPath(new URL('../components/IntCard.tsx', import.meta.url)), 'utf-8')

describe('badge single source (B14 D2)', () => {
  it('Dashboard no longer derives its status label from {enabled, healthy} alone', () => {
    expect(DASHBOARD_SRC).not.toContain("!enabled ? 'Disabled' : healthy ? 'Running' : 'Unhealthy'")
  })

  it('both the card and the tile import the shared badge helper', () => {
    expect(DASHBOARD_SRC).toContain("from '../lib/integrationBadge'")
    expect(INTCARD_SRC).toContain("from '../lib/integrationBadge'")
  })

  it('DashboardIntegration carries health, the field the tile used to discard', () => {
    expect(DASHBOARD_SRC).toMatch(/health:\s*HealthStatus/)
  })
})
