/**
 * Decision logic for the Pawns.app consent dialog.
 *
 * The Pawns.app CLI Addendum (§5.2–5.4) requires an explicit consent action
 * from the device owner before bandwidth sharing starts, and (§5.8) a durable
 * record of it. The record lives in the backend consent log — never in
 * localStorage, which the owner cannot be shown and an uninstall wipes.
 *
 * Kept as plain functions so the rules are testable without a DOM: the app has
 * no React testing library, and these are the parts worth proving.
 */

import { extractErrorMessage } from './error'

/**
 * Error string `toggle_integration` returns when Pawns.app is enabled without a
 * recorded consent. Matched verbatim by the UI to open the dialog instead of
 * showing a raw backend error, so it must stay in step with the Rust constant
 * of the same name in `commands/integration.rs`.
 */
export const PAWNS_CONSENT_REQUIRED = 'PAWNS_CONSENT_REQUIRED'

/** Backend `check_consent` reply. */
export interface ConsentStatus {
  integration_id: string
  active: boolean
  wording_version: string
  /** The audited disclosure text, rendered as-is — the UI never retypes it. */
  disclosure: string
  /** When the deciding consent/withdrawal was recorded, or null if never. */
  recorded_at: string | null
}

export type ConsentAction =
  | { kind: 'grant-then-toggle'; wordingVersion: string }
  | { kind: 'blocked' }

/** Only Pawns.app carries a consent requirement today. */
export function requiresConsent(id: string): boolean {
  return id === 'pawns'
}

/** The dialog's primary action stays disabled until the box is ticked. */
export function canConfirm(checked: boolean): boolean {
  return checked
}

/**
 * Whether enabling should stop and ask. An unreadable status opens the dialog
 * too: starting without being able to prove consent exists is the one outcome
 * the Addendum does not allow.
 */
export function shouldOpenDialog(status: ConsentStatus | null): boolean {
  return status === null || !status.active
}

/** Whether a rejected invoke is the backend asking for consent. */
export function isConsentRequiredError(err: unknown): boolean {
  if (err === null || err === undefined) return false
  return extractErrorMessage(err).includes(PAWNS_CONSENT_REQUIRED)
}

/**
 * What to do when the dialog is confirmed. The wording version comes from the
 * status the dialog rendered, so a dialog left open across an update is
 * rejected by the backend rather than recording consent to text the owner
 * never saw.
 */
export function nextActionAfterConfirm(status: ConsentStatus | null, checked: boolean): ConsentAction {
  if (!canConfirm(checked) || status === null) return { kind: 'blocked' }
  return { kind: 'grant-then-toggle', wordingVersion: status.wording_version }
}

/** The consent line on the Pawns card; null while the state is still unknown. */
export function consentBadge(active: boolean | null): { label: string; variant: 'run' | 'warn' } | null {
  if (active === null) return null
  return active
    ? { label: 'Consent active', variant: 'run' }
    : { label: 'Consent required', variant: 'warn' }
}
