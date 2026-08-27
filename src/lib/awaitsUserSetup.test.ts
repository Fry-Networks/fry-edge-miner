import { describe, it, expect } from 'vitest'
import { awaitsUserSetup, unhealthyReason } from './types'

// Storj reports Unhealthy while it waits for the operator to create a node
// auth token and finish node identity generation — a setup step that can take
// hours and that only the user can do. The card showed that as a red
// "Unhealthy" badge, which reads as a crash worth reporting rather than a
// to-do item, and Storj was reported as broken on that basis.

describe('awaitsUserSetup', () => {
  const storjReason =
    'Awaiting Storj setup — create a node auth token at storj.io and complete node identity to bring this node online. Install and eligibility are already active.'

  it('recognises the Storj awaiting-setup reason', () => {
    expect(awaitsUserSetup({ Unhealthy: storjReason })).toBe(true)
  })

  it('leaves a genuine failure classified as a failure', () => {
    expect(awaitsUserSetup({ Unhealthy: 'storagenode exited with code 1' })).toBe(false)
  })

  it('is false for every non-Unhealthy status', () => {
    expect(awaitsUserSetup('Healthy')).toBe(false)
    expect(awaitsUserSetup('Stopped')).toBe(false)
    expect(awaitsUserSetup('Starting')).toBe(false)
  })

  it('still exposes the full reason text for the card body', () => {
    // The badge changes; the explanation the user needs must survive.
    expect(unhealthyReason({ Unhealthy: storjReason })).toBe(storjReason)
  })
})
