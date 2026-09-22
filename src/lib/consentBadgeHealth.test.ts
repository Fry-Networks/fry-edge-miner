import { describe, it, expect } from 'vitest'
import { consentBadge } from './consentDialog'

// B18 D3: the card's consent badge was fetched only on mount and never
// re-checked, so after the backend lost consent (a supervisor restart
// wrongly recording a withdrawal — B18 D1/D2, T3-owned) the UI kept
// rendering "Consent active" beside a card whose health line already said
// "needs your consent" — the exact reported screenshot.

describe('consentBadge with a health reason (B18 D3)', () => {
  const NEEDS_CONSENT_REASON = 'Pawns.app needs your consent before it can share bandwidth — open it to review and enable.'

  it('never says active while the health reason says consent is required', () => {
    expect(
      (consentBadge as unknown as (a: boolean | null, h?: string | null) => unknown)(true, NEEDS_CONSENT_REASON)
    ).toEqual({ label: 'Consent required', variant: 'warn' })
  })

  it('is unchanged when no health reason is supplied (existing 2-arg call sites)', () => {
    expect(consentBadge(true)).toEqual({ label: 'Consent active', variant: 'run' })
    expect(consentBadge(false)).toEqual({ label: 'Consent required', variant: 'warn' })
    expect(consentBadge(null)).toBeNull()
  })

  it('an unrelated health reason does not override the active flag', () => {
    expect(
      (consentBadge as unknown as (a: boolean | null, h?: string | null) => unknown)(true, 'titan-edge process is not running')
    ).toEqual({ label: 'Consent active', variant: 'run' })
  })
})
