import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import * as types from './types'

const here = dirname(fileURLToPath(import.meta.url))
const intCard = readFileSync(resolve(here, '../components/IntCard.tsx'), 'utf-8')

const ADDR = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567ABCDEFGHIJKLMNOPQRSTUVWXYZ'
const FORBIDDEN = /activat|opt[ _-]?in\b|\bASA\b/i

describe('FEM-FRYDVPN-ALGO-CONFUSION: Fry dVPN CopyField, QR, 0.100 ALGO MBR & heartbeat guidance', () => {
  it('exports fryvpnFundingAddress, fryvpnAlgorandUri, and fryvpnFundingGuidance helpers', () => {
    const t = types as Record<string, unknown>
    expect(typeof t.fryvpnFundingAddress).toBe('function')
    expect(typeof t.fryvpnAlgorandUri).toBe('function')
    expect(typeof t.fryvpnFundingGuidance).toBe('function')

    const addrFn = t.fryvpnFundingAddress as (h: types.HealthStatus) => string | null
    const uriFn = t.fryvpnAlgorandUri as (h: types.HealthStatus) => string | null
    const guideFn = t.fryvpnFundingGuidance as (h: types.HealthStatus) => string | null

    const regReason = `Awaiting fryDVPN funding — send 0.312 ALGO to ${ADDR}, this device's own Fry dVPN node wallet (holds 0.000 ALGO). It needs 0.312 ALGO total for registration fees and minimum balance. Optional: not Fry's wallet or your reward wallet, and not a payment to anyone. To skip, switch Fry dVPN off. Olostep alone earns the full base reward; both add 15%.`
    expect(addrFn({ Unhealthy: regReason })).toBe(ADDR)
    expect(uriFn({ Unhealthy: regReason })).toBe(`algorand://${ADDR}?amount=312000`)
    const regGuide = guideFn({ Unhealthy: regReason }) ?? ''
    expect(regGuide).toContain('0.100 ALGO')
    expect(regGuide).not.toMatch(FORBIDDEN)

    const hbReason = `Fry dVPN is running, but this device's own Fry dVPN node wallet cannot pay the 0.001 ALGO fee of its next heartbeat. To keep it running, send 0.001000 ALGO to ${ADDR}. Optional: not Fry's wallet, and not a payment to anyone. To skip, switch Fry dVPN off. Olostep alone earns the full base reward; running both adds 15%.`
    expect(addrFn({ Unhealthy: hbReason })).toBe(ADDR)
    expect(uriFn({ Unhealthy: hbReason })).toBe(`algorand://${ADDR}?amount=1000`)
    const hbGuide = guideFn({ Unhealthy: hbReason }) ?? ''
    expect(hbGuide).toContain('0.001 ALGO')
    expect(hbGuide).toContain('0.100 ALGO')
    expect(hbGuide).not.toMatch(FORBIDDEN)
  })

  it('ships a pure-TS QR matrix generator', async () => {
    const qrPath = './qr'
    const qrMod = (await import(qrPath).catch(() => null)) as Record<string, unknown> | null
    expect(qrMod).not.toBeNull()
    expect(typeof qrMod?.encodeQrMatrix).toBe('function')
    const encode = qrMod!.encodeQrMatrix as (text: string) => boolean[][]
    const matrix = encode(`algorand://${ADDR}?amount=312000`)
    expect(matrix.length).toBeGreaterThanOrEqual(21)
    expect(matrix.length).toBe(matrix[0].length)
  })

  it('IntCard mounts fryvpn-fund-${id} outside sentinel-fund-..error-${id} with CopyField, FryvpnQr, and MBR guidance', () => {
    const sentinelStart = intCard.indexOf('sentinel-fund-')
    const errorSpan = intCard.indexOf('error-${id}', sentinelStart)
    const fryvpnStart = intCard.indexOf('fryvpn-fund-')
    expect(sentinelStart).toBeGreaterThan(-1)
    expect(errorSpan).toBeGreaterThan(sentinelStart)
    expect(fryvpnStart).toBeGreaterThan(errorSpan)

    const fryvpnBlock = intCard.slice(fryvpnStart, fryvpnStart + 1800)
    expect(fryvpnBlock).toContain('<CopyField val={fryvpnAddr}')
    expect(fryvpnBlock).toContain('<FryvpnQr')
    expect(fryvpnBlock).not.toMatch(FORBIDDEN)
  })
})
