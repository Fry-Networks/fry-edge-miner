import { describe, it, expect, vi } from 'vitest'
import { subscribeToHardeningStatus, type InvokeFn, type ListenFn } from './hardeningStatusEffect'

/**
 * BUG LOOP 3 (NB): before this file existed, the mount-time pull was pinned
 * only by `hardeningRetryWiring.test.ts`'s raw substring checks against
 * App.tsx's source text. Two mutants defeated every one of those checks
 * while actually breaking the boot-time Retry banner:
 *
 *   M1: delete `if (warning) setHardeningWarning(warning)` from the pull's
 *       `.then` (the live listener a few lines below still contains the
 *       literal text `setHardeningWarning(warning)`, so the substring check
 *       for it kept passing even with the pull's own call gone).
 *   M2: comment out the whole pull (`// invoke<string | null>(...)`.then(...)`)
 *       — `toContain` matches substrings inside comments too.
 *
 * This file drives `subscribeToHardeningStatus` with mocked `invoke`/
 * `listen`/`setWarning` and asserts the actual resulting call, so both
 * mutants are caught by a real behavioural failure instead of a text match.
 */

function makeInvoke(result: string | null): { invoke: InvokeFn; calls: string[] } {
  const calls: string[] = []
  const invoke = (<T,>(cmd: string) => {
    calls.push(cmd)
    return Promise.resolve(result as unknown as T)
  }) as InvokeFn
  return { invoke, calls }
}

function makeListen() {
  const unlisten = vi.fn()
  let registeredEvent: string | undefined
  let handler: ((event: { payload: unknown }) => void) | undefined
  const listen: ListenFn = (event, h) => {
    registeredEvent = event
    handler = h
    return Promise.resolve(unlisten)
  }
  return {
    listen,
    unlisten,
    fire: (payload: unknown) => handler?.({ payload }),
    getRegisteredEvent: () => registeredEvent
  }
}

describe('subscribeToHardeningStatus', () => {
  it('pulls get_hardening_status on mount and sets the warning when one is returned', async () => {
    const { invoke, calls } = makeInvoke('Defender exclusions declined')
    const { listen } = makeListen()
    const setWarning = vi.fn()

    subscribeToHardeningStatus({ invoke, listen, setWarning })
    await Promise.resolve()
    await Promise.resolve()

    expect(calls).toContain('get_hardening_status')
    expect(setWarning).toHaveBeenCalledTimes(1)
    expect(setWarning).toHaveBeenCalledWith({ reason: 'Defender exclusions declined', manualCommand: undefined })
  })

  it('does not set a warning when the pull reports nothing blocked', async () => {
    const { invoke } = makeInvoke(null)
    const { listen } = makeListen()
    const setWarning = vi.fn()

    subscribeToHardeningStatus({ invoke, listen, setWarning })
    await Promise.resolve()
    await Promise.resolve()

    expect(setWarning).not.toHaveBeenCalled()
  })

  it('registers a live elevation-required listener and sets the warning when it fires for hardening', async () => {
    const { invoke } = makeInvoke(null)
    const { listen, fire, getRegisteredEvent } = makeListen()
    const setWarning = vi.fn()

    subscribeToHardeningStatus({ invoke, listen, setWarning })
    await Promise.resolve()
    await Promise.resolve()

    expect(getRegisteredEvent()).toBe('elevation-required')
    fire({ purpose: 'hardening', reason: 'retry failed' })
    expect(setWarning).toHaveBeenCalledWith({ reason: 'retry failed', manualCommand: undefined })
  })

  it('ignores elevation-required events for other purposes', async () => {
    const { invoke } = makeInvoke(null)
    const { listen, fire } = makeListen()
    const setWarning = vi.fn()

    subscribeToHardeningStatus({ invoke, listen, setWarning })
    await Promise.resolve()
    await Promise.resolve()

    fire({ purpose: 'docker', reason: 'unrelated' })
    expect(setWarning).not.toHaveBeenCalled()
  })

  it('returns a cleanup function that calls the resolved unlisten', async () => {
    const { invoke } = makeInvoke(null)
    const { listen, unlisten } = makeListen()
    const setWarning = vi.fn()

    const cleanup = subscribeToHardeningStatus({ invoke, listen, setWarning })
    await Promise.resolve()
    await Promise.resolve()

    cleanup()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })
})
