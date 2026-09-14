/**
 * BUG 1/4: presentation helpers for the storage location.
 *
 * Kept out of JSX so they can be tested — this repo's vitest setup runs in a
 * `node` environment with no testing-library, so component internals are not
 * reachable from a test but pure functions are.
 */

export interface StorageLocation {
  path: string
  free_gb: number | null
  is_default: boolean
  pending_restart: boolean
}

/** `1863.4` -> `"1,863 GB free"`. Never renders `NaN GB`. */
export function formatFreeSpace(freeGb: number | null | undefined): string {
  if (freeGb === null || freeGb === undefined || !Number.isFinite(freeGb)) {
    return 'Free space unknown'
  }
  return `${Math.round(freeGb).toLocaleString('en-US')} GB free`
}

/** `"D:\FryEdgeMiner\partners"` -> `"D:"`. */
export function driveLabel(path: string | null | undefined): string | null {
  if (!path) return null
  const m = /^([A-Za-z]):/.exec(path.trim())
  return m ? `${m[1].toUpperCase()}:` : null
}

/** A change only takes effect on the next launch; say so while it is pending. */
export function pendingRestart(
  configured: string | null | undefined,
  active: string | null | undefined
): boolean {
  if (!configured || !active) return false
  return configured.trim().toLowerCase() !== active.trim().toLowerCase()
}

/**
 * Shown before a location change is written. The three load-bearing facts: it
 * takes effect on restart, nothing is moved or deleted, and the old data stops
 * being used.
 */
export const STORAGE_CHANGE_CONFIRM = (from: string, to: string): string =>
  `Change the storage location to ${to}?\n\n` +
  `This takes effect the next time Fry Edge Miner starts.\n\n` +
  `Nothing is moved or deleted. Files already written to ${from} stay there — ` +
  `including Space Acres plots and your Iagon node token — and Fry Edge Miner will not ` +
  `use them again. Partner binaries will be downloaded again into the new location, and ` +
  `Space Acres will start plotting from scratch.\n\n` +
  `Once you are happy the new location works, you can delete the old folder yourself.`
