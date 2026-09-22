import { describe, it, expect } from 'vitest'
import { awaitsUserSetup, unhealthyReason } from './types'

// B15 D5: a code-integrity/Smart App Control block (T2/T3-owned,
// integrations/code_integrity.rs) is a setup state the user must resolve
// (approve the blocked file), not a device fault the supervisor should
// restart-loop forever. Mirrors the existing awaitsUserSetup prefix-match
// pattern. src/lib/awaitsUserSetup.test.ts is not edited.

describe('awaitsUserSetup recognises a code-integrity / SAC block (B15 D5)', () => {
  const CI_REASON =
    'Awaiting administrator action — Windows blocked C:\\Users\\User\\AppData\\Roaming\\FryEdgeMiner\\partners\\titan\\goworkerd.dll from running. Review it in Windows Security and allow it if you trust it.'

  it('is true for the code-integrity block reason', () => {
    expect(awaitsUserSetup({ Unhealthy: CI_REASON })).toBe(true)
  })

  it('is false for an ordinary crash ("Error detected in daemon logs")', () => {
    expect(awaitsUserSetup({ Unhealthy: 'Error detected in daemon logs' })).toBe(false)
  })

  it('still exposes the full reason text for the card body', () => {
    expect(unhealthyReason({ Unhealthy: CI_REASON })).toBe(CI_REASON)
  })
})
