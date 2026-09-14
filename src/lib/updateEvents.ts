/**
 * BUG 10: giving the Updates page honest feedback.
 *
 * On Windows the app update never resolves: `download_and_install` hands off
 * via ShellExecuteW and calls `exit(0)`, so the awaited promise dies with the
 * process. The UI must therefore NEVER be built around a completion
 * resolution — it has to tell the user the app is about to close instead of
 * spinning forever on a promise that cannot settle.
 *
 * The backend already emits `update-failed` and `update-blocked-msi`; nothing
 * was listening.
 */

export type UpdatePhase = 'idle' | 'preparing' | 'handing-off' | 'blocked' | 'failed'

export interface UpdateBlockedPayload {
  reason?: string
  detail?: string
  uninstallString?: string
  message?: string
}

export interface UpdateFailedPayload {
  expected?: string
  actual?: string
  reason?: string
}

export function updateButtonState(p: { phase: UpdatePhase; available: boolean }): {
  disabled: boolean
  label: string
} {
  if (p.phase === 'preparing') return { disabled: true, label: 'Preparing…' }
  if (p.phase === 'handing-off') return { disabled: true, label: 'Installer running…' }
  if (p.phase === 'blocked') return { disabled: true, label: 'Update blocked' }
  if (!p.available) return { disabled: true, label: 'Up to date' }
  return { disabled: false, label: 'Update' }
}

/** Shown before the hand-off, because the app is about to disappear. */
export const UPDATE_HANDOFF_NOTICE =
  'Fry Edge Miner will close while the installer runs. Reopen it when the installer finishes.'

export function formatUpdateBlockedMsi(payload: UpdateBlockedPayload): {
  title: string
  detail: string
  command: string | null
} {
  const inconclusive = payload.reason === 'probe-inconclusive'
  return {
    title: inconclusive ? 'Update paused' : 'Update blocked',
    detail:
      payload.message ??
      (inconclusive
        ? 'Fry Edge Miner could not confirm how this copy was installed. It will try again automatically.'
        : 'This copy was installed from the .msi package, which the updater cannot safely replace.'),
    command: payload.uninstallString ? payload.uninstallString : null,
  }
}

export function formatUpdateFailed(payload: UpdateFailedPayload): {
  title: string
  detail: string
} {
  if (payload.expected && payload.actual) {
    return {
      title: 'Update did not complete',
      detail:
        `Fry Edge Miner expected version ${payload.expected} after updating but is running ` +
        `${payload.actual}. The previous version was kept — try updating again, and if it keeps ` +
        `failing check whether antivirus is blocking the installer.`,
    }
  }
  return {
    title: 'Update did not complete',
    detail: payload.reason ?? 'The update did not finish. The previous version was kept.',
  }
}
