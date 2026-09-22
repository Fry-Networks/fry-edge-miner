import { describe, it, expect } from 'vitest'
import { upstreamUnreachable, unhealthyReason } from './types'
import { integrationBadge } from './integrationBadge'

// B16 D4: a Titan-scheduler-unreachable network condition (titan.rs:270,
// T3-owned) painted the card as a red UNHEALTHY device fault. Mirrors the
// existing awaitsUserSetup prefix-match pattern. src/lib/awaitsUserSetup.test.ts
// is not edited.

const REASON =
  'Titan Network: cannot reach the Titan scheduler — this is a network              connection problem, not a fault on this device. Titan retries              automatically. Details: RPC client error: sendRequest failed: Post "https://cassini-locator.titannet.io:5000/rpc/v0": timeout: no recent network activity'

describe('upstreamUnreachable', () => {
  it('recognises the Titan scheduler-unreachable reason', () => {
    expect(upstreamUnreachable({ Unhealthy: REASON })).toBe(true)
  })

  it('leaves a genuine device fault classified as a failure', () => {
    expect(upstreamUnreachable({ Unhealthy: 'storagenode exited with code 1' })).toBe(false)
  })

  it('is false for every non-Unhealthy status', () => {
    expect(upstreamUnreachable('Healthy')).toBe(false)
    expect(upstreamUnreachable('Stopped')).toBe(false)
    expect(upstreamUnreachable('Starting')).toBe(false)
  })

  it('still exposes the full reason text for the card body', () => {
    expect(unhealthyReason({ Unhealthy: REASON })).toBe(REASON)
  })
})

// The shared badge ladder (B14) gets the new branch, between the
// awaitsUserSetup arm and the final Unhealthy else — same insertion point
// the original (now-extracted) IntCard.tsx ladder would have used.
describe('integrationBadge reads Upstream unreachable, amber not red (B16 D4)', () => {
  it('renders the new label with an info (not err) dot', () => {
    const badge = integrationBadge({
      enabled: true,
      healthy: false,
      health: { Unhealthy: REASON },
      lifecycle: 'Unhealthy',
      version: '1.0.0',
      unavailable_reason: null
    })
    expect(badge.label).toBe('Upstream unreachable')
    expect(badge.dot).toBe('info')
    expect(badge.tag).toBe('info')
  })

  it('a genuine failure still reads Unhealthy (control)', () => {
    const badge = integrationBadge({
      enabled: true,
      healthy: false,
      health: { Unhealthy: 'titan-edge process is not running: exit code 1' },
      lifecycle: 'Unhealthy',
      version: '1.0.0',
      unavailable_reason: null
    })
    expect(badge.label).toBe('Unhealthy')
  })
})
