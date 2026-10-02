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
  /**
   * B4 D3 + B13 D5: the storage root the app is ACTUALLY using this
   * session, when it differs from `path` because startup fell back to the
   * default (the configured root was unwritable). Optional — an older
   * backend simply omits it.
   */
  active_path?: string
  /** The reason a fallback happened, or null/absent when there was none. */
  fallback_reason?: string | null
  /** Simpler boolean twin of `fallback_reason` some backends may send
   *  instead (or in addition to) the detailed reason. */
  fell_back?: boolean
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

/**
 * B4 D3 + B13 D5: after a silent startup storage-root fallback, the
 * CONFIGURED root and the ACTIVE (in-use) root disagree — restarting cannot
 * fix an unavailable drive, so the ordinary pending_restart banner ("Restart
 * Fry Edge Miner to start using this location.") is actively false advice
 * here. Prefer the detailed `fallback_reason` shape; fall back to the
 * simpler boolean `fell_back` signal when only that is present. Returns
 * null when there is no fallback — the caller then falls through to the
 * genuine pending_restart case.
 */
export function storageFallbackMessage(
  storage: (Partial<StorageLocation> & Pick<StorageLocation, 'path'>) | null | undefined
): string | null {
  if (!storage) return null
  if (storage.fallback_reason) {
    return (
      `Fry Edge Miner could not write to ${storage.path} at startup (${storage.fallback_reason}) ` +
      `and is using ${storage.active_path ?? 'the default location'} for this session. ` +
      `Files at ${storage.path} have not been deleted.`
    )
  }
  if (storage.fell_back) {
    return `Fry Edge Miner could not write to ${storage.path} at startup and is using the default location for this session.`
  }
  return null
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
