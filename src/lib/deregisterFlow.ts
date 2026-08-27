// Deregistration UX state, kept out of the component so it can be tested
// without a DOM (the vitest environment here is `node`).
//
// The reported failure: Deregister looked like it did nothing. The server
// call failed, the rejection was swallowed into a generic error state the
// user never saw next to the button, and because the local clear only runs
// after a successful call, the key survived an uninstall and reinstall — the
// device kept coming back registered to the same key with no way out.

import { condenseError } from './error'

export const DEREGISTER_CONFIRM = 'Deregister this device? This cannot be undone.'

export const DEREGISTER_FORCE_CONFIRM =
  'The server did not accept the deregistration. Clear this device’s registration locally anyway? The server may still list it, and support may need to remove it there.'

export interface DeregisterState {
  /** Message to show beside the button, already condensed for one line. */
  error: string | null
  /** Whether to offer the local-only escape hatch. */
  offerForce: boolean
}

export const IDLE_DEREGISTER_STATE: DeregisterState = { error: null, offerForce: false }

/**
 * What the Settings page should show after a deregistration attempt fails.
 * `force` records whether the failed attempt was already the forced one — a
 * forced attempt that still fails has no further fallback to offer.
 */
export function deregisterFailureState(err: unknown, force: boolean): DeregisterState {
  return {
    error: condenseError(err instanceof Error ? err.message : String(err)),
    offerForce: !force
  }
}
