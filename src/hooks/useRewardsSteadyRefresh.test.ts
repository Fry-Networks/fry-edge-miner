import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, it, expect } from 'vitest'
import { shouldRefreshWhenReady, READY_REFRESH_MS, shouldPollAgain } from './useRewards'

// B22 D4: the reward summary was fetched once and never refreshed again
// once ready — it fossilised for the lifetime of the mount while the
// integration list (useIntegrations) keeps polling every 30s. A separate
// file from useRewardsPolling.test.ts (the not-ready-poll pinning) on
// purpose — that file is untouched.

describe('shouldRefreshWhenReady', () => {
  it('a ready summary must keep refreshing', () => {
    expect(shouldRefreshWhenReady(true, true)).toBe(true)
  })

  it('the not-ready path owns hasSummary && !ready', () => {
    expect(shouldRefreshWhenReady(true, false)).toBe(false)
  })

  it('nothing to refresh before the first fetch lands', () => {
    expect(shouldRefreshWhenReady(false, false)).toBe(false)
    expect(shouldRefreshWhenReady(false, true)).toBe(false)
  })
})

it('READY_REFRESH_MS matches useIntegrations.ts\'s 30s poll so the two ages stay matched', () => {
  expect(READY_REFRESH_MS).toBe(30_000)
})

it('the ready-refresh and not-ready-poll effects can never both be armed (complementarity)', () => {
  for (const hasSummary of [true, false]) {
    for (const ready of [true, false]) {
      const both = shouldPollAgain(hasSummary, ready, 0) && shouldRefreshWhenReady(hasSummary, ready)
      expect(both).toBe(false)
    }
  }
})

describe('useRewards.ts source guard', () => {
  const SRC = readFileSync(fileURLToPath(new URL('./useRewards.ts', import.meta.url)), 'utf-8')

  it('arms a steady-state interval once ready', () => {
    expect(SRC).toContain('setInterval(fetch, READY_REFRESH_MS)')
  })
})
