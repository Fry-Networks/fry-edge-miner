import { describe, it, expect } from 'vitest'
import { shouldConfirmDisable, DISABLE_CONFIRM_MS } from './disableConfirm'

describe('shouldConfirmDisable', () => {
  it('arms the caution row on the first click that would disable an official partner', () => {
    expect(shouldConfirmDisable({ tier: 'official', enabled: true, confirming: false })).toBe(true)
  })

  it('lets the second click through so the toggle actually happens', () => {
    // Without this the card would arm forever and the user could never
    // disable an official integration at all.
    expect(shouldConfirmDisable({ tier: 'official', enabled: true, confirming: true })).toBe(false)
  })

  it('never confirms enabling — there is no downside to warn about', () => {
    expect(shouldConfirmDisable({ tier: 'official', enabled: false, confirming: false })).toBe(false)
    expect(shouldConfirmDisable({ tier: 'official', enabled: false, confirming: true })).toBe(false)
  })

  it('never confirms an SDK toggle in either direction', () => {
    expect(shouldConfirmDisable({ tier: 'sdk', enabled: true, confirming: false })).toBe(false)
    expect(shouldConfirmDisable({ tier: 'sdk', enabled: false, confirming: false })).toBe(false)
    expect(shouldConfirmDisable({ tier: 'sdk', enabled: true, confirming: true })).toBe(false)
  })

  it('resets on a timer long enough to read the caution but short enough to forget', () => {
    expect(DISABLE_CONFIRM_MS).toBeGreaterThanOrEqual(3_000)
    expect(DISABLE_CONFIRM_MS).toBeLessThanOrEqual(10_000)
  })
})
