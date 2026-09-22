import { readFileSync } from 'node:fs'
import { describe, it, expect } from 'vitest'
import { integrationBadge } from './integrationBadge'

// B14 D1: the card's badge ladder tested `!inst` (version === null) BEFORE
// `healthy`, so an enabled+Healthy integration whose version probe happens to
// read null (e.g. a storage-root change without restarting the tracked
// child) rendered "Not installed" + "Auto-installs on enable" while the
// Dashboard tile rendered "Running". The regression row below is the
// refuter's repro case.

const SOURCE = readFileSync(new URL('./integrationBadge.ts', import.meta.url), 'utf-8')

describe('integrationBadge', () => {
  it('reads Running for an enabled+healthy card even when version is null (the regression row)', () => {
    const badge = integrationBadge({
      enabled: true,
      healthy: true,
      health: 'Healthy',
      lifecycle: 'Running',
      version: null,
      unavailable_reason: null,
      dockerBlocked: false
    })
    expect(badge.label).toBe('Running')
    expect(badge.dot).toBe('run')
    expect(badge.tag).toBe('run')
  })

  it('still reads Unavailable when docker-blocked and not installed (docker arm not shadowed)', () => {
    const badge = integrationBadge({
      enabled: false,
      healthy: false,
      health: 'Stopped',
      lifecycle: 'Disabled',
      version: null,
      unavailable_reason: null,
      dockerBlocked: true
    })
    expect(badge.label).toBe('Unavailable')
  })

  it('still reads Disabled for a disabled, installed integration (keeps integrations.spec.ts D3 green)', () => {
    const badge = integrationBadge({
      enabled: false,
      healthy: false,
      health: 'Stopped',
      lifecycle: 'Disabled',
      version: '0.0.0-preview',
      unavailable_reason: null,
      dockerBlocked: false
    })
    expect(badge.label).toBe('Disabled')
  })

  it('reads Not installed + warn tag for a genuinely uninstalled card', () => {
    const badge = integrationBadge({
      enabled: true,
      healthy: false,
      health: 'Stopped',
      lifecycle: 'Disabled',
      version: null,
      unavailable_reason: null,
      dockerBlocked: false
    })
    expect(badge.label).toBe('Not installed')
    expect(badge.tag).toBe('warn')
  })

  it('reads Installing with an info tag while an install is in flight', () => {
    const badge = integrationBadge({
      enabled: true,
      healthy: false,
      health: 'Stopped',
      lifecycle: 'Installing',
      version: null,
      unavailable_reason: null
    })
    expect(badge.kind).toBe('installing')
    expect(badge.label).toBe('Installing')
    expect(badge.tag).toBe('info')
  })

  it('reads Unavailable when the machine cannot meet requirements, regardless of docker', () => {
    const badge = integrationBadge({
      enabled: false,
      healthy: false,
      health: 'Stopped',
      lifecycle: 'Disabled',
      version: null,
      unavailable_reason: 'this device has 185 GB available'
    })
    expect(badge.label).toBe('Unavailable')
  })

  it('reads Starting for enabled+installed+not-yet-healthy transient states', () => {
    for (const health of ['Stopped', 'Starting', 'Unknown'] as const) {
      const badge = integrationBadge({
        enabled: true,
        healthy: false,
        health,
        lifecycle: 'Starting',
        version: '1.0.0',
        unavailable_reason: null
      })
      expect(badge.label).toBe('Starting')
      expect(badge.dot).toBe('info')
    }
  })

  it('reads Setup required for a partner waiting on a user step', () => {
    const badge = integrationBadge({
      enabled: true,
      healthy: false,
      health: { Unhealthy: 'Awaiting Storj setup — create a node auth token' },
      lifecycle: 'Unhealthy',
      version: '1.0.0',
      unavailable_reason: null
    })
    expect(badge.label).toBe('Setup required')
    expect(badge.dot).toBe('info')
  })

  it('reads Unhealthy for a genuine failure', () => {
    const badge = integrationBadge({
      enabled: true,
      healthy: false,
      health: { Unhealthy: 'titan-edge process is not running: exit code 3221225781' },
      lifecycle: 'Unhealthy',
      version: '1.0.0',
      unavailable_reason: null
    })
    expect(badge.label).toBe('Unhealthy')
    expect(badge.dot).toBe('err')
    expect(badge.tag).toBe('err')
  })
})

// D1's ordering guard: `enabled && healthy` must be tested before the plain
// `!inst` arm, or a null version probe shadows a genuinely running card.
describe('integrationBadge source ordering (D1 regression guard)', () => {
  it('places the enabled&&healthy arm before the bare !inst arm', () => {
    expect(SOURCE.indexOf("i.enabled && i.healthy")).toBeGreaterThan(-1)
    expect(SOURCE.indexOf('i.enabled && i.healthy')).toBeLessThan(SOURCE.indexOf('!inst) {'))
  })

  it('does not let the not-installed warn tag override a running badge', () => {
    expect(SOURCE).toMatch(/!inst\s*&&\s*dot\s*!==\s*'run'/)
  })
})
