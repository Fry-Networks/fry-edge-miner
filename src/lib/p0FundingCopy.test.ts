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

const fixtureReasonIn = (text: string, marker: string) => {
  const line = text.split(/\r?\n/).find((l) => l.includes(marker) && l.includes('Unhealthy:'))
  if (!line) throw new Error(`no fixture with ${marker}`)
  return line
}
const fixtureReason = (marker: string) => fixtureReasonIn(fixtures, marker)

// Render sentinel.rs's unfunded_message format literal with the constant-derived
// minimum and an address.
const renderUnfunded = (src: string) => {
  const literal = /format!\(\s*"(Sentinel node account not funded[\s\S]*?)",\s*Self::MIN_FUNDING_UDVPN/.exec(src)?.[1]
  expect(literal).toBeTruthy()
  const udvpn = Number(/MIN_FUNDING_UDVPN: u64 = (\d[\d_]*)/.exec(src)?.[1].replace(/_/g, ''))
  const args = [String(udvpn / 1_000_000), 'sent1qqqqexample']
  return literal!.replace(/\\\r?\n\s*/g, '').replace(/\{\}/g, () => args.shift()!)
}

describe('P0 Sentinel funding copy', () => {
  it('IntCard block is plain, optional, and derives the minimum from the backend text', () => {
    expect(sentinelBlock.length).toBeGreaterThan(100)
    expect(sentinelBlock).not.toMatch(FORBIDDEN)
    expect(sentinelBlock).toContain('Sentinel dVPN')
    expect(sentinelBlock).toContain("Sentinel network's token")
    expect(sentinelBlock).toContain('not a Fry token')
    expect(sentinelBlock).toContain('Optional')
    expect(sentinelBlock).toContain('register and pay Sentinel network fees')
    expect(intCard).toContain('sentinelFundingMinDvpn(health)')
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
    expect(line).toContain('register and pay Sentinel network fees')
    const udvpn = Number(/MIN_FUNDING_UDVPN: u64 = (\d[\d_]*)/.exec(sentinelRs)?.[1].replace(/_/g, ''))
    const reason = /Unhealthy: "(.*)" \}/.exec(line)![1]
    expect(sentinelFundingMinDvpn({ Unhealthy: reason })).toBe(String(udvpn / 1_000_000))
    expect(sentinelFundingAddress({ Unhealthy: reason })).toBe('sent1qqqqexample')
  })
})

describe('Rust wording and the IntCard parse stay tied (M1)', () => {
  it('renders sentinel.rs unfunded_message and parses it to MIN_FUNDING_UDVPN / 1e6', () => {
    const udvpn = Number(/MIN_FUNDING_UDVPN: u64 = (\d[\d_]*)/.exec(sentinelRs)?.[1].replace(/_/g, ''))
    const rendered = renderUnfunded(sentinelRs)
    const health = { Unhealthy: rendered }
    expect(sentinelFundingMinDvpn(health)).toBe(String(udvpn / 1_000_000))
    expect(sentinelFundingAddress(health)).toBe('sent1qqqqexample')
    // and the preview fixture is that exact text
    const fixture = /Unhealthy: "(.*)" \}/.exec(fixtureReason('node account not funded'))![1]
    expect(fixture).toBe(rendered)
  })
})

describe('parsers are EOL-agnostic (Windows CI checks out CRLF)', () => {
  // Normalise first: on a Windows checkout the source is already CRLF.
  const lf = (t: string) => t.replace(/\r\n/g, '\n')
  const crlf = (t: string) => lf(t).replace(/\n/g, '\r\n')
  it('renders the same unfunded text from a CRLF copy of sentinel.rs', () => {
    expect(renderUnfunded(crlf(sentinelRs))).toBe(renderUnfunded(lf(sentinelRs)))
  })
  it('finds the same fixture reason in a CRLF copy of useIntegrations.ts', () => {
    expect(fixtureReasonIn(crlf(fixtures), 'node account not funded')).toBe(fixtureReasonIn(lf(fixtures), 'node account not funded'))
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
    expect(line).toContain('Awaiting fryDVPN funding — send')
    expect(line).toContain('own Fry dVPN node wallet')
    expect(line).toContain('not a payment to anyone')
    expect(line).toContain('Olostep alone earns the full base reward')
    expect(line).toContain("Optional: not Fry's wallet")
    expect(line).toContain('To skip, switch Fry dVPN off.')
  })
})
