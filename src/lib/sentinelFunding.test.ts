import { describe, it, expect } from 'vitest'
import { sentinelFundingAddress } from './types'
import type { HealthStatus } from './types'

describe('sentinelFundingAddress', () => {
  it('extracts the sent1 address from the unfunded health message', () => {
    const health: HealthStatus = {
      Unhealthy:
        'Sentinel node account not funded — send DVPN to sent1m2cckurf9gkk74fgm2nd3har3ghxxz609prlpm to activate this node'
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
