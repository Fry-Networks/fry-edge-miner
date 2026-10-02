import type { HealthStatus } from './types'
import { unhealthyReason } from './types'

// B17 D2: TS mirror of src-tauri/src/integrations/mod.rs's AWAITING_MARKERS.
// Both lists must match exactly (see setupRequiredParity.test.ts) — a
// partner's Unhealthy reason means "waiting on a step only the user can do",
// not a fault the supervisor should restart-loop trying to recover from.
export const AWAITING_MARKERS = [
  'Awaiting Storj setup',
  'needs your consent',
  'node token not provisioned',
  'node account not funded',
  'Awaiting fryDVPN funding',
  'Awaiting administrator action',
  'waiting for that program to release it',
  'Click Retry on the Security hardening banner to allow fryDVPN through Windows Firewall.',
  'Finish setup in the SpaceAcres window to start earning.'
] as const

// c5 D-C5-1: an unconfigured SpaceAcres waits on its own setup wizard.
// Deliberately NOT in AWAITING_MARKERS (the parity-tested mirror of the Rust
// list); matched with `includes` because the supervisor may append
// "— automatic restarts paused; …" to the reason.
export const SPACE_ACRES_SETUP_TEXT = 'Finish setup in the SpaceAcres window to start earning.'

export function reasonAwaitsUserSetup(reason: string | null): boolean {
  return !!reason && (AWAITING_MARKERS.some((m) => reason.includes(m)) || reason.includes(SPACE_ACRES_SETUP_TEXT))
}

/**
 * B17 D4: a setup-blocked integration (Storj/Iagon/Sentinel waiting on the
 * user) is not a fleet failure — App.tsx's hasUnhealthy predicate painted
 * the sidebar amber for it anyway, because it only checked enabled+!healthy.
 * Strictly narrower: nothing that was non-alarming before becomes alarming.
 */
export function hasFailingIntegration(list: { enabled: boolean; healthy: boolean; health: HealthStatus }[]): boolean {
  return list.some((i) => i.enabled && !i.healthy && !reasonAwaitsUserSetup(unhealthyReason(i.health)))
}
