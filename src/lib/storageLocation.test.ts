import { describe, it, expect } from 'vitest'
import {
  formatFreeSpace,
  driveLabel,
  pendingRestart,
  STORAGE_CHANGE_CONFIRM,
} from './storageLocation'

// Windows paths are written with String.raw so a backslash can never be
// silently eaten by an escape sequence.
const D_PARTNERS = String.raw`D:\FryEdgeMiner\partners`
const C_PARTNERS = String.raw`C:\Users\u\AppData\Roaming\FryEdgeMiner\partners`
const UNC = String.raw`\\nas\share`

describe('formatFreeSpace', () => {
  it('formats a measured value with thousands separators', () => {
    expect(formatFreeSpace(1863.4)).toBe('1,863 GB free')
    expect(formatFreeSpace(71)).toBe('71 GB free')
    expect(formatFreeSpace(0)).toBe('0 GB free')
  })

  it('never renders NaN when the probe could not measure', () => {
    for (const v of [null, undefined, NaN, Infinity]) {
      const out = formatFreeSpace(v as number | null)
      expect(out).toBe('Free space unknown')
      expect(out).not.toContain('NaN')
    }
  })
})

describe('driveLabel', () => {
  it('extracts the volume the user is actually writing to', () => {
    expect(driveLabel(D_PARTNERS)).toBe('D:')
    expect(driveLabel('c:/x')).toBe('C:')
  })

  it('returns null when there is no drive letter', () => {
    expect(driveLabel('')).toBeNull()
    expect(driveLabel(UNC)).toBeNull()
    expect(driveLabel(null)).toBeNull()
  })
})

describe('pendingRestart', () => {
  it('is true only when the configured root differs from the active one', () => {
    // This is what the UI uses to say "restart to use this location".
    expect(pendingRestart(D_PARTNERS, C_PARTNERS)).toBe(true)
    expect(pendingRestart(D_PARTNERS, D_PARTNERS)).toBe(false)
  })

  it('ignores case, so a re-typed path does not look like a pending change', () => {
    expect(pendingRestart(D_PARTNERS.toUpperCase(), D_PARTNERS.toLowerCase())).toBe(false)
  })

  it('is false when either side is unknown', () => {
    expect(pendingRestart(null, C_PARTNERS)).toBe(false)
    expect(pendingRestart(C_PARTNERS, undefined)).toBe(false)
  })
})

describe('STORAGE_CHANGE_CONFIRM', () => {
  it('states the three things the user must know before confirming', () => {
    const msg = STORAGE_CHANGE_CONFIRM(C_PARTNERS, D_PARTNERS)
    expect(msg).toContain('next time')
    expect(msg).toContain('Nothing is moved or deleted')
    expect(msg).toContain(C_PARTNERS)
    expect(msg).toContain(D_PARTNERS)
  })
})
