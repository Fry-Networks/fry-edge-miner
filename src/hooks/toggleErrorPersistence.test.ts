import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, it, expect } from 'vitest'
import { shouldClearError } from './useIntegrations'

// B14 D3: runToggle's catch sets the error, then its own `finally` awaits
// fetch(), whose unconditional setError(null) erases it within one IPC
// round-trip while the same fetch resyncs the toggle back to OFF — a
// silent revert with no visible reason.

describe('shouldClearError', () => {
  it('clears the error on an ordinary successful poll', () => {
    expect(shouldClearError(true, false)).toBe(true)
  })

  it('does not clear an error the caller asked to preserve', () => {
    expect(shouldClearError(true, true)).toBe(false)
  })

  it('never clears on a failed fetch, preserve flag or not', () => {
    expect(shouldClearError(false, false)).toBe(false)
    expect(shouldClearError(false, true)).toBe(false)
  })
})

const SRC = readFileSync(fileURLToPath(new URL('./useIntegrations.ts', import.meta.url)), 'utf-8')

describe('toggle/reinstall error persistence source guard', () => {
  it('runToggle and forceReinstall both resync with preserveError so their own error is not erased mid-round-trip', () => {
    expect(SRC).toMatch(/await fetch\(\{ preserveError: true \}\)/)
    const hits = SRC.match(/await fetch\(\{ preserveError: true \}\)/g) ?? []
    expect(hits.length).toBe(2)
  })
})
