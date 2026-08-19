// F4: turning an official partner OFF costs the user reward proportion, so it
// takes two clicks. Not window.confirm — it is unreliable inside WebView2 —
// and not a dialog plugin, which is not a dependency of this app. The card
// shows a caution row instead and the second click within the window commits.

import type { IntegrationTier } from './integrationMeta'

/** How long the caution row stays armed before it resets itself. */
export const DISABLE_CONFIRM_MS = 6_000

export interface DisableConfirmState {
  tier: IntegrationTier
  /** Current state of the integration — the click is asking to invert it. */
  enabled: boolean
  /** Whether this card is already showing its caution row. */
  confirming: boolean
}

/**
 * Whether this click should arm the caution row instead of toggling.
 *
 * Only ever true for the first click that would disable an enabled official
 * partner. Enabling anything, and every SDK toggle, passes straight through —
 * a confirmation on an action with no downside is just friction.
 */
export function shouldConfirmDisable({ tier, enabled, confirming }: DisableConfirmState): boolean {
  if (tier !== 'official') return false
  if (!enabled) return false
  return !confirming
}
