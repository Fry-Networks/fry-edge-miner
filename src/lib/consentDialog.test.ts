import { describe, it, expect } from 'vitest'
import {
  PAWNS_CONSENT_REQUIRED,
  canConfirm,
  consentBadge,
  isConsentRequiredError,
  nextActionAfterConfirm,
  requiresConsent,
  shouldOpenDialog,
  type ConsentStatus
} from './consentDialog'

const status = (over: Partial<ConsentStatus> = {}): ConsentStatus => ({
  integration_id: 'pawns',
  active: false,
  wording_version: '1',
  disclosure: 'Pawns.app bandwidth sharing: …',
  recorded_at: null,
  ...over
})

describe('requiresConsent', () => {
  it('is true for pawns', () => {
    expect(requiresConsent('pawns')).toBe(true)
  })

  it('is false for every other integration', () => {
    for (const id of ['mysterium', 'storj', 'aem', 'iagon', 'fryvpn', '']) {
      expect(requiresConsent(id)).toBe(false)
    }
  })
})

describe('canConfirm', () => {
  it('blocks confirmation until the box is ticked', () => {
    expect(canConfirm(false)).toBe(false)
  })

  it('allows confirmation once the box is ticked', () => {
    expect(canConfirm(true)).toBe(true)
  })
})

describe('shouldOpenDialog', () => {
  it('opens when the backend has no recorded consent', () => {
    expect(shouldOpenDialog(status({ active: false }))).toBe(true)
  })

  it('stays closed when consent is already recorded', () => {
    expect(shouldOpenDialog(status({ active: true, recorded_at: '2026-08-19T10:00:00Z' }))).toBe(false)
  })

  it('opens when the status could not be read at all', () => {
    // Failing open would start sharing without a record; the dialog is the
    // safe direction when we cannot prove consent exists.
    expect(shouldOpenDialog(null)).toBe(true)
  })
})

describe('isConsentRequiredError', () => {
  it('recognises the backend sentinel as a plain string', () => {
    expect(isConsentRequiredError(PAWNS_CONSENT_REQUIRED)).toBe(true)
  })

  it('recognises the sentinel inside a wrapped Error', () => {
    expect(isConsentRequiredError(new Error(PAWNS_CONSENT_REQUIRED))).toBe(true)
  })

  it('recognises the sentinel inside a Tauri rejection object', () => {
    expect(isConsentRequiredError({ message: PAWNS_CONSENT_REQUIRED })).toBe(true)
  })

  it('does not mistake an ordinary start failure for a consent prompt', () => {
    expect(isConsentRequiredError('Could not start the Pawns.app agent: docker not running')).toBe(false)
    expect(isConsentRequiredError(new Error('Integration not found'))).toBe(false)
    expect(isConsentRequiredError(null)).toBe(false)
    expect(isConsentRequiredError(undefined)).toBe(false)
  })

  it('matches the exact sentinel the backend returns', () => {
    expect(PAWNS_CONSENT_REQUIRED).toBe('PAWNS_CONSENT_REQUIRED')
  })
})

describe('nextActionAfterConfirm', () => {
  it('grants consent and then toggles once the box is ticked', () => {
    expect(nextActionAfterConfirm(status(), true)).toEqual({
      kind: 'grant-then-toggle',
      wordingVersion: '1'
    })
  })

  it('carries the backend wording version through so a stale dialog is rejected', () => {
    expect(nextActionAfterConfirm(status({ wording_version: '2' }), true)).toEqual({
      kind: 'grant-then-toggle',
      wordingVersion: '2'
    })
  })

  it('does nothing while the box is unticked', () => {
    expect(nextActionAfterConfirm(status(), false)).toEqual({ kind: 'blocked' })
  })

  it('does nothing when there is no status to consent against', () => {
    expect(nextActionAfterConfirm(null, true)).toEqual({ kind: 'blocked' })
  })
})

describe('consentBadge', () => {
  it('reports active consent in the running style', () => {
    expect(consentBadge(true)).toEqual({ label: 'Consent active', variant: 'run' })
  })

  it('reports missing consent in the caution style', () => {
    expect(consentBadge(false)).toEqual({ label: 'Consent required', variant: 'warn' })
  })

  it('shows nothing until the status is known', () => {
    expect(consentBadge(null)).toBeNull()
  })
})
