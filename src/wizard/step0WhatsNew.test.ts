import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'

// A1 (rc39): the wizard's "What's new" bullets were 0.2.x-era text. The
// bullets are a literal array in Step0Welcome.tsx, so scan the source.

const src = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), './Step0Welcome.tsx'), 'utf-8')
const block = /const whatsNew = \[([\s\S]*?)\n  \]/.exec(src)?.[1] ?? ''
const bullets = [...block.matchAll(/'((?:[^'\\]|\\.)*)'/g)].map((m) => m[1])

describe('Step0Welcome "What\'s new"', () => {
  it('drops the stale 0.2.x-era bullets', () => {
    for (const stale of [
      'Clearer registration errors',
      'DNS/carrier-block',
      'Degraded (local issue)',
      'Registration retries transient server errors',
      'Accessibility: toggles',
      '87 end-to-end'
    ]) {
      expect(src).not.toContain(stale)
    }
  })

  it('lists 4-6 short 0.4.39 items', () => {
    expect(bullets.length).toBeGreaterThanOrEqual(4)
    expect(bullets.length).toBeLessThanOrEqual(6)
    for (const b of bullets) expect(b.length).toBeLessThanOrEqual(140)
    const all = bullets.join('\n')
    expect(all).toContain('Fry dVPN and Sentinel dVPN funding')
    expect(all).toContain('Security hardening no longer asks again')
    expect(all).toContain('Diiisco')
  })
})
