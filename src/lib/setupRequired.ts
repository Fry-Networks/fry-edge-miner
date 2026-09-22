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
  'Awaiting administrator action'
] as const

export function reasonAwaitsUserSetup(reason: string | null): boolean {
  return !!reason && AWAITING_MARKERS.some((m) => reason.includes(m))
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
