import { describe, expect, test } from 'vitest'
import { deriveConnectivity } from './connectivity'

// Repro for the v0.4.7 field reports: users without Docker running saw a
// permanent "Degraded" badge and read it as a broken connection to Fry.
// Docker state is a LOCAL concern (surfaced per integration card + Docker
// chip) and must never affect the connectivity badge.
describe('deriveConnectivity', () => {
  test('docker daemon_stopped alone does NOT degrade the badge', () => {
    expect(
      deriveConnectivity({ deviceError: null, integrationsError: null, dockerStatus: 'daemon_stopped' })
    ).toBe('connected')
  })

  test('docker not_installed alone does NOT degrade the badge', () => {
    expect(
      deriveConnectivity({ deviceError: null, integrationsError: null, dockerStatus: 'not_installed' })
    ).toBe('connected')
  })

  test('docker virtualization_disabled alone does NOT degrade the badge', () => {
    expect(
      deriveConnectivity({ deviceError: null, integrationsError: null, dockerStatus: 'virtualization_disabled' })
    ).toBe('connected')
  })

  test('integrations fetch error degrades the badge (docker ready)', () => {
    expect(
      deriveConnectivity({ deviceError: null, integrationsError: 'ipc failed', dockerStatus: 'ready' })
    ).toBe('degraded')
  })

  test('device error wins: disconnected', () => {
    expect(
      deriveConnectivity({ deviceError: 'api down', integrationsError: null, dockerStatus: 'ready' })
    ).toBe('disconnected')
  })

  test('all clear: connected', () => {
    expect(
      deriveConnectivity({ deviceError: null, integrationsError: null, dockerStatus: 'ready' })
    ).toBe('connected')
  })

  test('unknown system status (null): connected when no errors', () => {
    expect(
      deriveConnectivity({ deviceError: null, integrationsError: null, dockerStatus: null })
    ).toBe('connected')
  })
})
