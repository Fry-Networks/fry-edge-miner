/**
 * Whether Settings should surface the SAVED miner key as a copyable field
 * even though the device is not (or no longer) registered (v0.4.8 — field
 * reports showed 'Not registered' with the saved key hidden, so users
 * couldn't read or copy the identity their device had been mining under).
 */
export function shouldShowSavedMinerKey(
  isRegistered: boolean,
  minerKey: string | null | undefined
): boolean {
  return !isRegistered && !!minerKey
}
