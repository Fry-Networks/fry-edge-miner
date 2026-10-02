import { describe, it, expect } from 'vitest'
import { hardeningWarningFromEventPayload, hardeningWarningFromStatus } from './hardeningWarning'

describe('hardeningWarningFromEventPayload', () => {
  it('decodes a hardening elevation-required payload', () => {
    const warning = hardeningWarningFromEventPayload({
      purpose: 'hardening',
      reason: 'Needs administrator approval — Retry',
      manualCommand: 'powershell -Command "..."'
    })
    expect(warning).toEqual({
      reason: 'Needs administrator approval — Retry',
      manualCommand: 'powershell -Command "..."'
    })
  })

  it('manualCommand is optional (the updater pre-update re-assert never sends one)', () => {
    const warning = hardeningWarningFromEventPayload({
      purpose: 'hardening',
      reason: 'hardening setup failed: exit 2'
    })
    expect(warning).toEqual({ reason: 'hardening setup failed: exit 2', manualCommand: undefined })
  })

  it('ignores an elevation-required payload for a DIFFERENT purpose', () => {
    // The same event name is used for other integrations' elevation state
    // (merged into their own cards via elevation_gate::blocked_reasons) —
    // this must not steal those.
    expect(
      hardeningWarningFromEventPayload({ purpose: 'docker-desktop', reason: 'Needs administrator approval — Retry' })
    ).toBeNull()
  })

  it('rejects malformed payloads without throwing', () => {
    expect(hardeningWarningFromEventPayload(null)).toBeNull()
    expect(hardeningWarningFromEventPayload(undefined)).toBeNull()
    expect(hardeningWarningFromEventPayload('hardening')).toBeNull()
    expect(hardeningWarningFromEventPayload({ purpose: 'hardening' })).toBeNull()
    expect(hardeningWarningFromEventPayload({ purpose: 'hardening', reason: '' })).toBeNull()
    expect(hardeningWarningFromEventPayload({ purpose: 'hardening', reason: 42 })).toBeNull()
  })
})

describe('hardeningWarningFromStatus', () => {
  it('decodes a pulled blocked-reason string', () => {
    expect(hardeningWarningFromStatus('Needs administrator approval — Retry')).toEqual({
      reason: 'Needs administrator approval — Retry',
      manualCommand: undefined
    })
  })

  it('null/undefined/empty means nothing is currently blocked', () => {
    expect(hardeningWarningFromStatus(null)).toBeNull()
    expect(hardeningWarningFromStatus(undefined)).toBeNull()
    expect(hardeningWarningFromStatus('')).toBeNull()
  })
})
