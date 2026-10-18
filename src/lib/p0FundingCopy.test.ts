import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import { sentinelFundingMinDvpn, sentinelFundingAddress } from './types'

// P0 (rc39): plain-language, optional funding copy. Source-scan of the
// user-visible Sentinel / fryDVPN funding strings in the UI (the Rust
// messages are covered by p0_funding_copy_tests.rs / sentinel_p0_copy_tests.rs).

const here = dirname(fileURLToPath(import.meta.url))
const read = (rel: string) => readFileSync(resolve(here, rel), 'utf-8')
const intCard = read('../components/IntCard.tsx')
const meta = read('./integrationMeta.ts')
const fixtures = read('../hooks/useIntegrations.ts')
const sentinelRs = read('../../src-tauri/src/integrations/sentinel.rs')

const FORBIDDEN = /activat|opt[ _-]?in\b|\bASA\b/i

const sentinelBlock = (() => {
  const at = intCard.indexOf('sentinel-fund-')
  const end = intCard.indexOf('error-${id}', at)
  return intCard.slice(at, end)
})()

const sentinelMeta = (() => {
  const at = meta.indexOf('sentinel: {')
  return meta.slice(at, meta.indexOf('}', at))
})()

const fixtureReason = (marker: string) => {
  const line = fixtures.split('\n').find((l) => l.includes(marker) && l.includes('Unhealthy:'))
  if (!line) throw new Error(`no fixture with ${marker}`)
  return line
}

describe('P0 Sentinel funding copy', () => {
  it('IntCard block is plain, optional, and derives the minimum from the backend text', () => {
    expect(sentinelBlock.length).toBeGreaterThan(100)
    expect(sentinelBlock).not.toMatch(FORBIDDEN)
    expect(sentinelBlock).toContain('Sentinel dVPN')
    expect(sentinelBlock).toContain("Sentinel network's token")
    expect(sentinelBlock).toContain('not a Fry token')
    expect(sentinelBlock).toContain('Optional')
    expect(sentinelBlock).toContain('does not affect your other rewards')
    expect(sentinelBlock).toContain('sentinelFundingMinDvpn')
    expect(sentinelBlock).not.toMatch(/\b10 DVPN/)
  })

  it('integrationMeta says what the account is for, without activation wording', () => {
    expect(sentinelMeta).not.toMatch(FORBIDDEN)
    expect(sentinelMeta).toContain('Sentinel dVPN')
    expect(sentinelMeta).toContain('not a payment to Fry')
  })

  it('preview fixture mirrors the backend text and its minimum equals the Rust constant', () => {
    const line = fixtureReason('node account not funded')
    expect(line).not.toMatch(FORBIDDEN)
    expect(line).toContain('Sentinel dVPN')
    expect(line).toContain("Sentinel network's token")
    expect(line).toContain('does not affect your other rewards')
    const udvpn = Number(/MIN_FUNDING_UDVPN: u64 = (\d[\d_]*)/.exec(sentinelRs)?.[1].replace(/_/g, ''))
    const reason = /Unhealthy: '(.*)' \}/.exec(line)![1]
    expect(sentinelFundingMinDvpn({ Unhealthy: reason })).toBe(String(udvpn / 1_000_000))
    expect(sentinelFundingAddress({ Unhealthy: reason })).toBe('sent1qqqqexample')
  })
})

describe('sentinelFundingMinDvpn', () => {
  it('parses the minimum out of the backend reason', () => {
    expect(sentinelFundingMinDvpn({ Unhealthy: 'Sentinel node account not funded — needs at least 25 DVPN to register' })).toBe('25')
  })
  it('is null when the reason carries no figure', () => {
    expect(sentinelFundingMinDvpn({ Unhealthy: 'Sentinel container exited' })).toBeNull()
    expect(sentinelFundingMinDvpn('Healthy')).toBeNull()
  })
})

describe('P0 fryDVPN funding fixture', () => {
  it('mirrors the live card copy', () => {
    const line = fixtureReason('Awaiting fryDVPN funding')
    expect(line).not.toMatch(FORBIDDEN)
    expect(line).toContain('Awaiting fryDVPN funding — Optional:')
    expect(line).toContain('own Fry dVPN node wallet')
    expect(line).toContain('not a payment to anyone')
    expect(line).toContain('Olostep alone earns the full base reward')
    expect(line).toContain('turn Fry dVPN off with its switch')
  })
})
