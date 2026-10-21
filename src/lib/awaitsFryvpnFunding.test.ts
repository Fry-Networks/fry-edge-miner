import { describe, it, expect } from 'vitest'
import { awaitsUserSetup, unhealthyReason } from './types'

// B7 D4: fryvpn's unaffordable-wallet message rendered as a red UNHEALTHY
// device fault instead of the amber "Setup required" state — the message
// (fryvpn.rs, prefixed "Awaiting fryDVPN funding —", T3-owned) matched
// neither of the existing awaits-user-setup markers. New marker added to
// src/lib/setupRequired.ts's AWAITING_MARKERS (T3 makes the matching
// addition on the Rust side; both sides are compared by
// setupRequiredParity.test.ts). Deliberately a separate file —
// awaitsUserSetup.test.ts is not edited.

describe('awaitsUserSetup recognises the fryDVPN funding-needed reason (B7 D4)', () => {
  const FUNDING =
    "Awaiting fryDVPN funding — send 0.212 ALGO to ADDR, this device's own Fry dVPN node wallet (holds 0.100 ALGO). It needs 0.312 ALGO total for registration fees and minimum balance. Optional: not Fry's wallet or your reward wallet, and not a payment to anyone. To skip, switch Fry dVPN off. Olostep alone earns the full base reward; both add 15%."

  it('is true for the funding-needed reason', () => {
    expect(awaitsUserSetup({ Unhealthy: FUNDING })).toBe(true)
  })

  it('is false for an unrelated frynode failure', () => {
    expect(awaitsUserSetup({ Unhealthy: 'frynode process is not running: exit code 1' })).toBe(false)
  })

  it('still exposes the full reason text for the card body', () => {
    expect(unhealthyReason({ Unhealthy: FUNDING })).toBe(FUNDING)
  })
})
