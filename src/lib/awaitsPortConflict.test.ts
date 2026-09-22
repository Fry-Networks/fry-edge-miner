import { describe, it, expect } from 'vitest'
import { awaitsUserSetup, unhealthyReason } from './types'

// B10 (T3-owned, integrations/port_conflict.rs): frynode failing to start
// because another program already holds its TCP port is not a fault FEM's
// supervisor can fix by restarting — nothing FEM restarts can take a port
// away from another program. Mirrors the marker parity pattern used for
// B7/B15/B17's other "waiting on something outside FEM" reasons.

describe('awaitsUserSetup recognises a held-port reason (B10)', () => {
  const HELD_BY =
    'frynode could not start: TCP port 8088 is already in use by some-other.exe (PID 4242) — waiting for that program to release it'
  const HELD_UNKNOWN =
    'frynode could not start: TCP port 8088 is already in use by another program — waiting for that program to release it'

  it('is true for both held-port reason shapes', () => {
    expect(awaitsUserSetup({ Unhealthy: HELD_BY })).toBe(true)
    expect(awaitsUserSetup({ Unhealthy: HELD_UNKNOWN })).toBe(true)
  })

  it('is false for an unrelated frynode failure', () => {
    expect(awaitsUserSetup({ Unhealthy: 'frynode process is not running: exit code 1' })).toBe(false)
  })

  it('still exposes the full reason text for the card body', () => {
    expect(unhealthyReason({ Unhealthy: HELD_BY })).toBe(HELD_BY)
  })
})
