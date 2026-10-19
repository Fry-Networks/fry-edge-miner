import { describe, it, expect } from 'vitest'
import { sentinelFundingAddress } from './types'
import type { HealthStatus } from './types'

describe('sentinelFundingAddress', () => {
  it('extracts the sent1 address from the unfunded health message', () => {
    const health: HealthStatus = {
      Unhealthy:
        "Sentinel node account not funded — Optional: this device's own Sentinel node account needs at least 10 DVPN to register and pay Sentinel network fees. DVPN is the Sentinel network's token, not a Fry token. Send it to sent1m2cckurf9gkk74fgm2nd3har3ghxxz609prlpm. Not a payment to Fry or anyone. To skip it, turn Sentinel dVPN off with its switch."
    }
    expect(sentinelFundingAddress(health)).toBe('sent1m2cckurf9gkk74fgm2nd3har3ghxxz609prlpm')
  })

  it('returns null for an unhealthy message with no sent1 address', () => {
    expect(sentinelFundingAddress({ Unhealthy: 'Sentinel container exited' })).toBeNull()
  })

  it('returns null for a healthy status', () => {
    expect(sentinelFundingAddress('Healthy')).toBeNull()
  })

  it('returns null for a non-unhealthy status object-free value', () => {
    expect(sentinelFundingAddress('Starting')).toBeNull()
  })
})
