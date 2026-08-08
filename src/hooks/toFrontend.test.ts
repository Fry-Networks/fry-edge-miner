import { describe, it, expect } from 'vitest'
import { toFrontend } from './useIntegrations'
import type { IntegrationStatus } from '../lib/types'

// The backend already reports why a start failed — toggle_integration stores the
// message in last_integration_error and get_integrations returns it as `error`.
// It used to be dropped here, which is why enabling fryvpn or diiisco flipped the
// switch back with nothing shown anywhere.

const status = (over: Partial<IntegrationStatus> = {}): IntegrationStatus => ({
  id: 'fryvpn',
  display_name: 'Fry dVPN',
  enabled: false,
  health: 'Stopped',
  lifecycle: 'Disabled',
  version: '0.1.0',
  poc_contribution: 0,
  ...over
})

describe('toFrontend', () => {
  it('carries a start error through to the card model', () => {
    const msg = 'Failed to spawn frynode: The system cannot find the file specified. (os error 2)'
    const [out] = toFrontend([status({ error: msg })])
    expect(out.error).toBe(msg)
  })

  it('carries a docker-specific error through for diiisco', () => {
    const msg = 'docker compose build failed: error during connect'
    const [out] = toFrontend([status({ id: 'diiisco', display_name: 'Diiisco', error: msg })])
    expect(out.error).toBe(msg)
  })

  it('is null when the backend reports no error', () => {
    const [out] = toFrontend([status()])
    expect(out.error).toBeNull()
  })

  it('does not confuse a start error with a hardware-unavailable reason', () => {
    // Both can be present; they are different conditions and render differently.
    const [out] = toFrontend([
      status({ id: 'iagon', error: 'boom', unavailable_reason: 'Iagon requires at least 900 GB' })
    ])
    expect(out.error).toBe('boom')
    expect(out.unavailable_reason).toBe('Iagon requires at least 900 GB')
  })

  it('still maps the fields it already handled', () => {
    const [out] = toFrontend([status({ enabled: true, health: 'Healthy', requires_docker: true })])
    expect(out.enabled).toBe(true)
    expect(out.healthy).toBe(true)
    expect(out.requires_docker).toBe(true)
    expect(out.name).toBe('Fry dVPN')
  })
})
