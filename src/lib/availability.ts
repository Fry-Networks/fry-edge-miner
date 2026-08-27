// Availability rules for integrations whose minimum specs this machine cannot
// meet. Kept out of the components so they can be tested without a DOM — the
// vitest environment here is `node`, with no testing-library.

/** Minimal shape these rules need; the real type is FrontendIntegration. */
export interface AvailabilityLike {
  id: string
  enabled: boolean
  unavailable_reason?: string | null
}

/** True when the backend reported a reason this machine cannot run it. */
export function isUnavailable(intg: AvailabilityLike): boolean {
  return !!intg.unavailable_reason
}

/**
 * Which members a category's toggle-all should actually flip.
 *
 * Turning a category ON skips unavailable members: the backend refuses to
 * start them, and a rejection the user did not ask for reads as a bug.
 * Turning one OFF must still include an unavailable member that is currently
 * enabled — otherwise an integration that became unavailable while running
 * (Diiisco, once its device wallet stops resolving) can never be switched off
 * from the category slider. The backend's disable path applies no
 * requirements gate, so a stop is always accepted.
 *
 * Members already in the target state are skipped either way — re-toggling
 * them would flip them back.
 */
export function toggleAllTargets<T extends AvailabilityLike>(members: T[], next: boolean): T[] {
  return members.filter((i) => (next ? !isUnavailable(i) : true) && i.enabled !== next)
}

/**
 * Counts behind the category badge. `availableTotal` excludes unavailable
 * members so "2/3 active" never implies a slot the user could fill.
 */
export function categoryCounts(members: AvailabilityLike[]): {
  activeCount: number
  availableTotal: number
  unavailableCount: number
} {
  const unavailableCount = members.filter(isUnavailable).length
  return {
    activeCount: members.filter((i) => i.enabled).length,
    availableTotal: members.length - unavailableCount,
    unavailableCount
  }
}
