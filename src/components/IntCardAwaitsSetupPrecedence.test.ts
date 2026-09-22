import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, it, expect } from 'vitest'

// Cross-team fix requested by T3 (via lead) for B7 D4 / B8 D3-D4: today an
// amber "Setup required" card renders with NO body text in two situations,
// which hides the funding amount/address (and — a real, independent bug —
// already hides the existing Storj setup instruction):
//
// 1. A STALE lastError (from an earlier failed toggle attempt) outranks a
//    LIVE awaitsUserSetup(health) reason, because startError's guard does
//    not know about it.
// 2. The reason-explanation render gate can, in an edge case the ladder's
//    precedence creates (e.g. !inst or !enabled evaluated before the
//    awaitsUserSetup branch — see lib/integrationBadge.ts), be false even
//    though awaitsUserSetup(health) is true — stLbl only equals
//    'Setup required' when no earlier-priority arm fired first.
//
// Source-guard idiom (IntCardWarnings.test.ts) — vitest env is `node`, no
// jsdom/testing-library. Real rendering is proven in
// tests/e2e/awaits-setup-precedence.spec.ts.

const SOURCE = readFileSync(fileURLToPath(new URL('./IntCard.tsx', import.meta.url)), 'utf-8')

describe('a live awaitsUserSetup reason outranks a stale start error (IntCard.tsx)', () => {
  it('startError is suppressed while health is awaiting user setup', () => {
    expect(SOURCE).toMatch(/const startError = !unavailable && !awaitsUserSetup\(health\) && lastError/)
  })

  it('the reason-explanation render gate also opens directly on awaitsUserSetup(health)', () => {
    // Additive to the existing st==='err' / stLbl==='Setup required' /
    // stLbl==='Upstream unreachable' clauses — closes the ladder-precedence
    // edge case where stLbl isn't literally 'Setup required' even though
    // awaitsUserSetup(health) is true.
    expect(SOURCE).toMatch(/st === 'err' \|\| stLbl === 'Setup required' \|\| stLbl === 'Upstream unreachable' \|\| awaitsUserSetup\(health\)/)
  })

  it('IntCard.tsx imports awaitsUserSetup directly again (needed by both gates above)', () => {
    expect(SOURCE).toMatch(/import\s*\{[^}]*awaitsUserSetup[^}]*\}\s*from\s*'\.\.\/lib\/types'/)
  })
})
