/**
 * FAIL-11 (row 6): the backend emits `elevation-required` whenever an
 * AUTOMATIC hardening attempt (boot, or the pre-update re-assert) is
 * suppressed by the gate or declined/fails — but until this fix nothing in
 * the frontend listened for it, so the only place hardening's blocked state
 * ever appeared was a log line. `event_payload` is the pure decoder for that
 * event: it accepts the raw Tauri event payload and returns a typed warning
 * ONLY when it is actually about hardening (other purposes use the same
 * event name for other integrations' elevation state, via
 * `elevation_gate::blocked_reasons` merged into their own cards — this must
 * not intercept those).
 */
export interface HardeningWarning {
  reason: string
  manualCommand?: string
}

export function hardeningWarningFromEventPayload(payload: unknown): HardeningWarning | null {
  if (typeof payload !== 'object' || payload === null) return null
  const p = payload as Record<string, unknown>
  if (p.purpose !== 'hardening') return null
  if (typeof p.reason !== 'string' || p.reason.length === 0) return null
  return {
    reason: p.reason,
    manualCommand: typeof p.manualCommand === 'string' ? p.manualCommand : undefined
  }
}
