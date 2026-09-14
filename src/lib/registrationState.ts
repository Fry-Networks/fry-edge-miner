/**
 * BUG 10/RC4: telling "registered" apart from "finished registering".
 *
 * `registered` deliberately keeps its historic meaning (a miner key exists), so
 * that a half-registered device is NOT routed back into the wizard. The new
 * `registration_complete` flag is what distinguishes a device the server has
 * actually confirmed from one whose registration call failed and left it with a
 * key but no install id.
 */

export type RegistrationBadge = 'registered' | 'finishing' | 'unregistered'

export interface RegistrationLike {
  registered: boolean
  /** Optional: an older backend does not send it. */
  registration_complete?: boolean
}

/**
 * `registration_complete === undefined` means the backend predates this field —
 * fall back to the old two-state view rather than showing every user a
 * "finishing" badge that will never resolve.
 */
export function deriveRegistrationBadge(d: RegistrationLike | null | undefined): RegistrationBadge {
  if (!d || !d.registered) return 'unregistered'
  if (d.registration_complete === undefined) return 'registered'
  return d.registration_complete ? 'registered' : 'finishing'
}

export function registrationLabel(badge: RegistrationBadge): string {
  switch (badge) {
    case 'registered':
      return 'Registered'
    case 'finishing':
      return 'Finishing registration…'
    case 'unregistered':
      return 'Not registered'
  }
}

/**
 * A device stuck mid-registration should still show its miner key — the key is
 * the one thing the user needs in order to get help.
 */
export function shouldShowMinerKey(badge: RegistrationBadge, minerKey: string | null | undefined): boolean {
  return !!minerKey && badge !== 'unregistered'
}
