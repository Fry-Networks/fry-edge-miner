import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, it, expect } from 'vitest'
import { formatReward, formatRewardWithToken } from './formatReward'

// B2 red case: the reward history table rendered `${r.reward} ${token}`, so a
// server-summed double printed its full float tail — "13.392000000000001 FRY".

describe('formatReward', () => {
  it('renders the float artifact from the field report as the value it means', () => {
    expect(String(13.392000000000001)).toBe('13.392000000000001') // what the table used to show
    expect(formatReward(13.392000000000001)).toBe('13.392')
  })

  it('trims the padding toFixed adds, including on whole numbers', () => {
    expect(formatReward(5)).toBe('5')
    expect(formatReward(1000)).toBe('1000')
    expect(formatReward(10.5)).toBe('10.5')
  })

  it('keeps significant zeros inside the number', () => {
    expect(formatReward(100.05)).toBe('100.05')
    expect(formatReward(0.000001)).toBe('0.000001')
  })

  it('rounds at six decimals rather than showing float noise', () => {
    expect(formatReward(0.1 + 0.2)).toBe('0.3')
    expect(formatReward(1.0000004)).toBe('1')
    expect(formatReward(2.9999999)).toBe('3')
  })

  it('handles zero, negatives and non-finite input without producing junk', () => {
    expect(formatReward(0)).toBe('0')
    expect(formatReward(-0.0000001)).toBe('0')
    expect(formatReward(-13.392000000000001)).toBe('-13.392')
    expect(formatReward(Number.NaN)).toBe('0')
    expect(formatReward(Number.POSITIVE_INFINITY)).toBe('0')
  })

  it('appends the token symbol for the history table', () => {
    expect(formatRewardWithToken(13.392000000000001, 'FRY')).toBe('13.392 FRY')
  })
})

// The helper only fixes anything if the table actually calls it. The vitest
// environment here is `node` with no testing-library, so the wiring is proved
// against the source itself rather than a render.
describe('Rewards history table wiring', () => {
  const source = readFileSync(
    fileURLToPath(new URL('../pages/Rewards.tsx', import.meta.url)),
    'utf8'
  )

  it('imports the shared formatter', () => {
    expect(source).toMatch(/from '\.\.\/lib\/formatReward'/)
  })

  it('no longer interpolates a raw reward into the table cell', () => {
    // The pre-fix expression, verbatim.
    expect(source).not.toContain('${r.reward} ${rewardToken}')
  })

  it('formats every reward it renders', () => {
    expect(source).toMatch(/formatRewardWithToken\(r\.reward, rewardToken\)/)
  })
})
