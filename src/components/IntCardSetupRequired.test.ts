import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, it, expect } from 'vitest'
import { SETUP_GUIDANCE } from '../lib/integrationMeta'

// B17 D5: the setup-required state rendered NO guidance text — the reason
// block was gated on `st === 'err'`, so once D1/D2 move Sentinel (and
// Storj/Iagon) to st === 'info' ("Setup required"), that gate goes false and
// the existing Sentinel funding block (CopyField, the sent1 address, the
// role="alert" region) disappears in exactly the state that needs it most.
// Source-guard idiom (IntCardWarnings.test.ts) — vitest env is `node`, no
// jsdom/testing-library.

const SOURCE = readFileSync(fileURLToPath(new URL('./IntCard.tsx', import.meta.url)), 'utf-8')

describe('IntCard setup-required guidance (B17 D5)', () => {
  it('widens the reason gate to also open for Setup required, not just st === err', () => {
    // The naive/original gate must be gone in isolation...
    expect(SOURCE).not.toMatch(/\{reason && st === 'err' && !startError && \(/)
    // ...replaced by a gate that also opens on the Setup required label.
    expect(SOURCE).toMatch(/st === 'err' \|\| stLbl === 'Setup required'/)
  })

  it('keeps every existing child of the reason block mounted (Sentinel funding block untouched)', () => {
    expect(SOURCE).toContain('data-testid={`sentinel-fund-${id}`}')
    expect(SOURCE).toContain('<CopyField val={fundingAddr} />')
    expect(SOURCE).toContain('role="alert"')
  })

  it('renders setup guidance (what + link) for integrations that have it', () => {
    expect(SOURCE).toContain('SETUP_GUIDANCE')
  })

  it('no SETUP_GUIDANCE copy makes an unverified cost claim', () => {
    for (const g of Object.values(SETUP_GUIDANCE)) {
      expect(g.what).not.toMatch(/free|no cost|costs?\s*(nothing|money)|\$/i)
    }
  })

  it('SETUP_GUIDANCE covers the three partners that can reach Setup required', () => {
    expect(Object.keys(SETUP_GUIDANCE).sort()).toEqual(['iagon', 'sentinel', 'storj'])
    for (const g of Object.values(SETUP_GUIDANCE)) {
      expect(g.url).toMatch(/^https:\/\//)
      expect(g.urlLabel.length).toBeGreaterThan(0)
      expect(g.what.length).toBeGreaterThan(0)
    }
  })
})
