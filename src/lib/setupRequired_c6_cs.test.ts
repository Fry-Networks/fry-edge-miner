import { describe, it, expect } from 'vitest'
import { AWAITING_MARKERS, reasonAwaitsUserSetup, hasFailingIntegration } from './setupRequired'
import { integrationBadge } from './integrationBadge'

// D-C6-4: the fryDVPN firewall instruction and the SpaceAcres setup text are
// AWAITING_MARKERS literals, mirrored from the Rust list. The card's firewall
// line also carries "Awaiting administrator action", so the BARE sentence is
// used here: it is recognised only through the new literal.

const FIREWALL = 'Click Retry on the Security hardening banner to allow fryDVPN through Windows Firewall.'
const SPACE_ACRES = 'Finish setup in the SpaceAcres window to start earning.'

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

describe('fryDVPN firewall setup line (D-C6-4)', () => {
  it('reads Setup required on its own, not as a failure', () => {
    expect(reasonAwaitsUserSetup(FIREWALL)).toBe(true)
    expect(badgeFor(FIREWALL).label).toBe('Setup required')
    expect(hasFailingIntegration([{ enabled: true, healthy: false, health: { Unhealthy: FIREWALL } }])).toBe(false)
  })

  it('leaves an ordinary frynode fault a failure', () => {
    const fault = 'frynode process is not running'
    expect(reasonAwaitsUserSetup(fault)).toBe(false)
    expect(hasFailingIntegration([{ enabled: true, healthy: false, health: { Unhealthy: fault } }])).toBe(true)
  })
})

describe('SpaceAcres setup text in AWAITING_MARKERS (D-C6-4)', () => {
  // Pin only: SPACE_ACRES_SETUP_TEXT already makes this reason read Setup
  // required, so the behaviour cannot change on this side.
  it('is one of the mirrored markers', () => {
    expect(AWAITING_MARKERS).toContain(SPACE_ACRES)
  })
})
