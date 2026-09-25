import { hardeningWarningFromEventPayload, hardeningWarningFromStatus, type HardeningWarning } from './hardeningWarning'

/**
 * BUG LOOP 3 (NB): the mount-time reachability fix (BUG LOOP 2) was pinned
 * on the frontend only by raw substring scans of App.tsx's SOURCE TEXT
 * (`hardeningRetryWiring.test.ts`). A comment can satisfy a substring check,
 * and `setHardeningWarning(warning)` — the pull's actual effect — already
 * occurs verbatim in the unrelated live-listener code a few lines below, so
 * deleting the pull's own `if (warning) setHardeningWarning(warning)` line,
 * or commenting the whole pull out, left every substring test green while
 * silently reopening "the Retry banner never appears at boot".
 *
 * This module is the mount effect's logic, extracted out of App.tsx and
 * parameterized over `invoke`/`listen`/`setWarning`, so it can be exercised
 * for REAL — with mocked dependencies, not string-matched — and a deleted
 * state update or a dropped pull call actually fails the test instead of
 * merely disappearing from the substring scan.
 */

/** The minimal shape of `@tauri-apps/api/core`'s `invoke` this module needs. */
export type InvokeFn = <T>(cmd: string) => Promise<T>

/** The minimal shape of `@tauri-apps/api/event`'s `listen` this module needs. */
export type ListenFn = (
  event: string,
  handler: (event: { payload: unknown }) => void
) => Promise<() => void>

export interface HardeningStatusEffectDeps {
  invoke: InvokeFn
  listen: ListenFn
  setWarning: (warning: HardeningWarning) => void
}

/**
 * Wires the boot-reachability pull (`get_hardening_status`, queried once on
 * mount — the boot pass's Automatic refusal can publish into the gate
 * microseconds after setup, long before this ever runs, and Tauri's `emit`
 * is fire-and-forget with no replay, so a block that happened before this
 * mounted would otherwise be lost) together with the live
 * `elevation-required` listener (catches a LATER change — a Retry, or the
 * updater's pre-update re-assert — while mounted).
 *
 * Returns the cleanup function React's `useEffect` expects.
 */
export function subscribeToHardeningStatus(deps: HardeningStatusEffectDeps): () => void {
  deps
    .invoke<string | null>('get_hardening_status')
    .then((reason) => {
      const warning = hardeningWarningFromStatus(reason)
      if (warning) deps.setWarning(warning)
    })
    .catch(() => {})

  let unlisten: (() => void) | undefined
  deps
    .listen('elevation-required', (event) => {
      const warning = hardeningWarningFromEventPayload(event.payload)
      if (warning) deps.setWarning(warning)
    })
    .then((f) => {
      unlisten = f
    })
    .catch(() => {})

  return () => unlisten?.()
}
