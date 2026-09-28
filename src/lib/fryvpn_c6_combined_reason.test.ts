import { describe, it, expect } from 'vitest'
import { reasonAwaitsUserSetup, hasFailingIntegration } from './setupRequired'
import { awaitsUserSetup, upstreamUnreachable, sentinelFundingAddress, unhealthyReason } from './types'
import { integrationBadge } from './integrationBadge'
import { consentBadge } from './consentDialog'

// D-C6-1: a registered fryDVPN node waiting on its firewall rule whose wallet
// cannot pay its next heartbeat shows the setup line, then " · " and the
// heartbeat shortfall. Every card-text matcher must treat that combined line
// exactly as it treats the setup line alone.

const SETUP =
  'Awaiting administrator action — Click Retry on the Security hardening banner to allow fryDVPN through Windows Firewall.'
const COMBINED =
  SETUP +
  " · This node's wallet also cannot pay the 0.001 ALGO fee of its next heartbeat — send 0.000001 ALGO to YTC4NR6IZHFMXTGNZ3H5BUOS2PKNLVWX3DM5VW643XPN7YHB4LRUS2CHMA."

function card(reason: string) {
  return {
    enabled: true,
    healthy: false,
    health: { Unhealthy: reason },
    lifecycle: 'Unhealthy' as const,
    version: '1.0.0',
    unavailable_reason: null,
    dockerBlocked: false
  }
}

describe('D-C6-1 combined setup + heartbeat line', () => {
  for (const [name, reason] of [
    ['setup line alone', SETUP],
    ['combined line', COMBINED]
  ] as const) {
    it(`${name}: reads Setup required everywhere`, () => {
      expect(reasonAwaitsUserSetup(reason)).toBe(true)
      expect(awaitsUserSetup({ Unhealthy: reason })).toBe(true)
      const badge = integrationBadge(card(reason))
      expect(badge.kind).toBe('setupRequired')
      expect(badge.label).toBe('Setup required')
      expect(hasFailingIntegration([card(reason)])).toBe(false)
      expect(upstreamUnreachable({ Unhealthy: reason })).toBe(false)
      expect(sentinelFundingAddress({ Unhealthy: reason })).toBeNull()
      expect(consentBadge(null, reason)).toBeNull()
    })
  }

  it('keeps the full combined text for the card body', () => {
    expect(unhealthyReason({ Unhealthy: COMBINED })).toBe(COMBINED)
  })
})
