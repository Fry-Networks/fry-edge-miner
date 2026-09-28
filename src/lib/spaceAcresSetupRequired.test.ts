import { describe, it, expect } from 'vitest'
import { awaitsUserSetup } from './types'
import { hasFailingIntegration } from './setupRequired'
import { integrationBadge } from './integrationBadge'

// c5 D-C5-1: an unconfigured SpaceAcres reports this exact reason, and its
// card must read SETUP REQUIRED, not a red failure. The text is matched on
// the frontend only, outside AWAITING_MARKERS (which mirrors the Rust
// supervisor's list and is parity-tested), and by `includes`, because the
// supervisor appends "— automatic restarts paused; …" once it stops retrying.

const SETUP = 'Finish setup in the SpaceAcres window to start earning.'
const PAUSED = `${SETUP} — automatic restarts paused; the next automatic attempt is in 300s`

function badgeFor(reason: string) {
  return integrationBadge({
    enabled: true,
    healthy: false,
    health: { Unhealthy: reason },
    lifecycle: 'Unhealthy',
    version: '1.0.0',
    unavailable_reason: null
  })
}

describe('SpaceAcres setup-required reason (D-C5-1)', () => {
  it('reads Setup required, plain and with the paused suffix', () => {
    for (const reason of [SETUP, PAUSED]) {
      expect(awaitsUserSetup({ Unhealthy: reason })).toBe(true)
      expect(badgeFor(reason).label).toBe('Setup required')
      expect(hasFailingIntegration([{ enabled: true, healthy: false, health: { Unhealthy: reason } }])).toBe(false)
    }
  })

  it('leaves a SpaceAcres fault a failure', () => {
    for (const reason of ['SpaceAcres process is not running', 'Finish setup in the SpaceAcres window']) {
      expect(awaitsUserSetup({ Unhealthy: reason })).toBe(false)
      expect(badgeFor(reason).label).not.toBe('Setup required')
    }
  })
})
