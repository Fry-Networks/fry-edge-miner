import { describe, it, expect } from 'vitest'
import {
  deriveRegistrationBadge,
  registrationLabel,
  shouldShowMinerKey,
} from './registrationState'

describe('deriveRegistrationBadge', () => {
  it('shows a half-registered device as finishing, not as done', () => {
    // BUG 10/RC4: this is the device that had a miner key but no install id and
    // silently rendered the full app as though everything was fine.
    expect(deriveRegistrationBadge({ registered: true, registration_complete: false })).toBe(
      'finishing'
    )
  })

  it('shows a fully registered device as registered', () => {
    expect(deriveRegistrationBadge({ registered: true, registration_complete: true })).toBe(
      'registered'
    )
  })

  it('shows an unregistered device as unregistered', () => {
    expect(deriveRegistrationBadge({ registered: false, registration_complete: false })).toBe(
      'unregistered'
    )
    expect(deriveRegistrationBadge(null)).toBe('unregistered')
    expect(deriveRegistrationBadge(undefined)).toBe('unregistered')
  })

  it('never shows "finishing" against a backend that does not send the field', () => {
    // Version skew must not strand the user on a badge that can never resolve.
    expect(deriveRegistrationBadge({ registered: true })).toBe('registered')
    expect(deriveRegistrationBadge({ registered: false })).toBe('unregistered')
  })
})

describe('registrationLabel', () => {
  it('gives every badge a distinct human label', () => {
    const labels = (['registered', 'finishing', 'unregistered'] as const).map(registrationLabel)
    expect(new Set(labels).size).toBe(3)
    expect(registrationLabel('finishing')).toContain('Finishing')
  })
})

describe('shouldShowMinerKey', () => {
  it('keeps the key visible while registration is still finishing', () => {
    // The key is what the user needs in order to ask for help.
    expect(shouldShowMinerKey('finishing', 'FEM-ABC')).toBe(true)
    expect(shouldShowMinerKey('registered', 'FEM-ABC')).toBe(true)
  })

  it('hides it when there is nothing to show', () => {
    expect(shouldShowMinerKey('unregistered', 'FEM-ABC')).toBe(false)
    expect(shouldShowMinerKey('finishing', null)).toBe(false)
    expect(shouldShowMinerKey('finishing', '')).toBe(false)
  })
})
