import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'

// P0b (rc39b): every user-visible funding text is at most 60 words and keeps
// its required elements. The Rust texts are covered by p0b_copy_len_tests.rs;
// this covers the UI card and the preview fixtures that mirror the live copy.

const here = dirname(fileURLToPath(import.meta.url))
const read = (rel: string) => readFileSync(resolve(here, rel), 'utf-8')
const intCard = read('../components/IntCard.tsx')
const fixtures = read('../hooks/useIntegrations.ts')
const FORBIDDEN = /activat|opt[ _-]?in\b|\bASA\b/i
const words = (t: string) => t.split(/\s+/).filter(Boolean).length

const fixtureReason = (marker: string) => {
  const line = fixtures.split(/\r?\n/).find((l) => l.includes(marker) && l.includes('Unhealthy:'))
  if (!line) throw new Error(`no fixture with ${marker}`)
  return /Unhealthy: "(.*)" \}/.exec(line)![1]
}

// The two IntCard lines that show together on the Sentinel funding card
// (template literals with the minimum substituted by its "at least N DVPN" form).
const cardLines = () => {
  const at = intCard.indexOf("Optional: Sentinel dVPN's own node account")
  const first = intCard.slice(at, intCard.indexOf('address:', at) + 'address:'.length)
  const bat = intCard.indexOf('Buy DVPN', at)
  const second = intCard.slice(bat, intCard.indexOf('`', bat))
  return [first.replace(/\$\{minDvpn \? `at least \$\{minDvpn\} DVPN` : 'DVPN'\}/, 'at least 10 DVPN'), second]
}

describe('P0b copy length', () => {
  it('the fryDVPN funding card fixture is at most 60 words with every element', () => {
    const t = fixtureReason('Awaiting fryDVPN funding')
    expect(words(t)).toBeLessThanOrEqual(60)
    expect(t).not.toMatch(FORBIDDEN)
    expect(t.startsWith('Awaiting fryDVPN funding — send 0.212 ALGO to ADDR')).toBe(true)
    expect(t).toContain("device's own Fry dVPN node wallet (holds 0.100 ALGO)")
    expect(t).toContain('It needs 0.312 ALGO total')
    expect(t).toContain('registration fees')
    expect(t).toContain('minimum balance')
    expect(t).toContain('not a payment to anyone')
    expect(t).toContain("not Fry's wallet")
    expect(t).toContain('Optional')
    expect(t).toContain('switch Fry dVPN off')
    expect(t).toContain('Olostep alone earns the full base reward')
    expect(t).toContain('15%')
  })

  it('the Sentinel fixture is at most 60 words with every element', () => {
    const t = fixtureReason('node account not funded')
    expect(words(t)).toBeLessThanOrEqual(60)
    expect(t).not.toMatch(FORBIDDEN)
    expect(t).toContain("this device's own Sentinel node account")
    expect(t).toContain('register and pay Sentinel network fees')
    expect(t).toContain("Sentinel network's token, not a Fry token")
    expect(t).toContain('Not a payment to Fry or anyone')
    expect(t).toContain('Optional')
    expect(t).toContain('turn Sentinel dVPN off with its switch')
  })

  it('the Sentinel IntCard lines together are at most 60 words with every element', () => {
    const [a, b] = cardLines()
    const t = `${a} ${b}`
    expect(words(t)).toBeLessThanOrEqual(60)
    expect(t).not.toMatch(FORBIDDEN)
    expect(t).toContain("Sentinel dVPN's own node account")
    expect(t).toContain('register and pay Sentinel network fees')
    expect(t).toContain("Sentinel network's token, not a Fry token")
    expect(t).toContain('not a payment to Fry')
    expect(t).toContain('Optional')
    expect(t).toContain('turn Sentinel dVPN off with its switch')
  })
})
