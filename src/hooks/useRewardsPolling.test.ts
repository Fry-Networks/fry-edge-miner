import { describe, it, expect } from 'vitest'
import { shouldPollAgain, NOT_READY_POLL_CAP } from './useRewards'

// useRewards used to fetch the reward summary exactly once on mount. If that
// fetch landed before the PoC-loop's first tick warmed the cache, the stale
// (not-ready) summary sat there forever — the only way to get real numbers
// was to navigate away and back, remounting the hook. shouldPollAgain is the
// pure decision behind the bounded not-ready re-poll that replaces that.

describe('shouldPollAgain', () => {
  it('does not poll before any summary has been fetched', () => {
    expect(shouldPollAgain(false, false, 0)).toBe(false)
  })

  it('polls again after a not-ready response', () => {
    expect(shouldPollAgain(true, false, 0)).toBe(true)
  })

  it('stops once the summary is ready', () => {
    expect(shouldPollAgain(true, true, 0)).toBe(false)
  })

  it('keeps polling right up to the cap', () => {
    expect(shouldPollAgain(true, false, NOT_READY_POLL_CAP - 1)).toBe(true)
  })

  it('stops at the hard cap even if still not ready', () => {
    expect(shouldPollAgain(true, false, NOT_READY_POLL_CAP)).toBe(false)
  })
})
