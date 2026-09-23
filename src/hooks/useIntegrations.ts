import { useCallback, useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { DockerProgress, HealthStatus, IntegrationStatus, LifecycleState, SystemStatus } from '../lib/types'
import {
  isConsentRequiredError,
  nextActionAfterConfirm,
  requiresConsent,
  shouldOpenDialog,
  type ConsentStatus
} from '../lib/consentDialog'
import { extractErrorMessage } from '../lib/error'
import { INTEGRATION_META, type IntegrationMeta } from '../lib/integrationMeta'
import { isTauri } from '../lib/tauri'

function deriveLifecycle(enabled: boolean, health: HealthStatus): LifecycleState {
  if (!enabled) return 'Disabled'
  if (health === 'Healthy') return 'Running'
  if (typeof health !== 'string' && 'Unhealthy' in health) return 'Unhealthy'
  if (health === 'Installing') return 'Installing'
  return 'Starting'
}

// B14 D2 / B17 D-12: browser-preview-only test hook, mirrors the existing
// `?docker=<kind>` (fetchSystem, below) and useDevice's `?wizard=1`. Lets a
// Playwright spec drive an individual mock integration into any badge state
// without a live backend, e.g. `?intg=titan:running&intg=storj:setupRequired`.
export type IntgHintState =
  | 'running'
  | 'starting'
  | 'disabled'
  | 'notInstalled'
  | 'unavailable'
  | 'installing'
  | 'setupRequired'
  | 'setupRequiredFunding'
  | 'unhealthy'
  | 'needsConsent'
  | 'setupRequiredWithStaleError'

export function intgHintOverrides(state: string): Partial<IntegrationStatus> {
  switch (state as IntgHintState) {
    case 'running':
      return { enabled: true, health: 'Healthy', lifecycle: 'Running', version: '1.0.0' }
    case 'starting':
      return { enabled: true, health: 'Starting', lifecycle: 'Starting', version: '1.0.0' }
    case 'disabled':
      return { enabled: false, health: 'Stopped', lifecycle: 'Disabled', version: '1.0.0' }
    case 'notInstalled':
      return { enabled: false, health: 'Stopped', lifecycle: 'Disabled', version: null }
    case 'unavailable':
      return {
        enabled: false,
        health: 'Stopped',
        lifecycle: 'Disabled',
        version: null,
        unavailable_reason: 'this device does not meet requirements'
      }
    case 'installing':
      return { enabled: true, health: 'Stopped', lifecycle: 'Installing', version: null }
    case 'setupRequired':
      return {
        enabled: true,
        health: { Unhealthy: 'Awaiting Storj setup — create a node auth token' },
        lifecycle: 'Unhealthy',
        version: '1.0.0'
      }
    case 'setupRequiredFunding':
      // Verbatim shape of the Sentinel unfunded-account reason (sentinel.rs)
      // — carries a sent1 address so sentinelFundingAddress() extracts it.
      return {
        enabled: true,
        health: { Unhealthy: 'Sentinel node account not funded — send DVPN to sent1qqqqexample to activate this node' },
        lifecycle: 'Unhealthy',
        version: '1.0.0'
      }
    case 'unhealthy':
      return { enabled: true, health: { Unhealthy: 'container exited: panic' }, lifecycle: 'Unhealthy', version: '1.0.0' }
    case 'needsConsent':
      // B18 D3: the verbatim Pawns "needs your consent" health reason —
      // exercises consentBadge's health-reason override independent of the
      // separate (backend-driven, not reachable in browser preview)
      // consentActive flag.
      return {
        enabled: true,
        health: { Unhealthy: 'Pawns.app needs your consent before it can share bandwidth — open it to review and enable.' },
        lifecycle: 'Unhealthy',
        version: '1.0.0'
      }
    case 'setupRequiredWithStaleError':
      // Cross-team fix for B7 D4/B8 D3-D4: a STALE `error` from an earlier
      // failed toggle attempt used to outrank a LIVE awaitsUserSetup health
      // reason, hiding the funding/setup guidance body text entirely.
      return {
        enabled: true,
        health: { Unhealthy: 'Awaiting fryDVPN funding — fryDVPN needs about 0.312 ALGO in this device\'s wallet to register on-chain, and it currently has 0.100 ALGO — about 0.212 ALGO short. Send ALGO to ADDR and fryDVPN will register automatically on the next check.' },
        lifecycle: 'Unhealthy',
        version: '1.0.0',
        error: 'frynode could not be started: a previous attempt timed out'
      }
    default:
      return {}
  }
}

// B14 D3: runToggle/forceReinstall set a specific error, then their own
// `finally` resyncs with the backend by calling fetch(). fetch()'s success
// path unconditionally cleared the error, erasing it within one IPC
// round-trip — the toggle appeared to silently revert with no explanation.
// preserveError lets a caller resync state without wiping the error it just
// set; an ordinary poll (no caller opts in) still clears a stale error.
export function shouldClearError(fetchOk: boolean, preserveError: boolean): boolean {
  return fetchOk && !preserveError
}

export interface FrontendIntegration extends IntegrationMeta {
  enabled: boolean
  health: HealthStatus
  healthy: boolean
  lifecycle: LifecycleState
  version: string | null
  poc_contribution: number
  requires_docker: boolean
  /** Why this machine cannot run it, or null when it can. */
  unavailable_reason: string | null
  /** Why the last enable attempt failed, or null. */
  error: string | null
}

export function toFrontend(integrations: IntegrationStatus[]): FrontendIntegration[] {
  return integrations.map((i) => {
    const base = INTEGRATION_META.find((m) => m.id === i.id)
    const enabled = i.enabled
    const health = i.health
    const lifecycle =
      i.lifecycle && typeof i.lifecycle === 'string' ? i.lifecycle : deriveLifecycle(enabled, health)
    return {
      id: i.id,
      name: i.display_name || base?.name || i.id,
      tag: base?.tag || 'NODE',
      desc: base?.desc || '',
      Icon: base?.Icon || INTEGRATION_META[0].Icon,
      col: base?.col || '#00c49a',
      uptime: base?.uptime ?? 0,
      // Unknown ids fall into "AI & Data" so a backend-only integration still
      // renders somewhere instead of vanishing from the grouped view.
      category: base?.category ?? 'AI & Data',
      // Backend is authoritative; the meta table covers an older backend that
      // does not send `tier` yet. An id neither side knows is treated as
      // community, never as an official partner.
      tier: i.tier ?? base?.tier ?? 'sdk',
      enabled,
      health,
      healthy: health === 'Healthy',
      lifecycle,
      version: i.version,
      // Backend value is healthy-based (matches what the PoC reporter
      // actually submits); the equal-split fallback only covers browser
      // preview mode without IPC.
      poc_contribution: i.poc_contribution ?? (integrations.length > 0 ? 1 / integrations.length : 0),
      requires_docker: i.requires_docker ?? false,
      unavailable_reason: i.unavailable_reason ?? null,
      error: i.error ?? null
    }
  })
}

export function useIntegrations() {
  const [integrations, setIntegrations] = useState<FrontendIntegration[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [system, setSystem] = useState<SystemStatus | null>(null)
  const [dockerProgress, setDockerProgress] = useState<DockerProgress | null>(null)
  // Consent state for the integrations that need one (Pawns.app today): the
  // open dialog, and the per-integration flag the card badge reads.
  const [consentPrompt, setConsentPrompt] = useState<ConsentStatus | null>(null)
  const [consentBusy, setConsentBusy] = useState(false)
  const [consentActive, setConsentActive] = useState<Record<string, boolean>>({})
  // Toggles currently awaiting the backend — the docker-progress banner is
  // only cleared when NO toggle is in flight, so one integration's failure
  // can't hide another's still-running Docker install.
  const inflightToggles = useRef(0)

  const fetch = useCallback(async (opts?: { preserveError?: boolean }) => {
    if (!isTauri()) {
      // Browser preview mode — show all integrations with mock data
      const intgHints = new Map(
        new URLSearchParams(window.location.search).getAll('intg').map((pair) => {
          const [id, state] = pair.split(':')
          return [id, state] as const
        })
      )
      const mock: IntegrationStatus[] = INTEGRATION_META.map((m) => ({
        id: m.id,
        display_name: m.name,
        enabled: false,
        health: 'Stopped' as HealthStatus,
        lifecycle: 'Disabled' as LifecycleState,
        version: '0.0.0-preview',
        poc_contribution: 1 / INTEGRATION_META.length,
        requires_docker: true,
        ...(intgHints.has(m.id) ? intgHintOverrides(intgHints.get(m.id)!) : {})
      }))
      setIntegrations(toFrontend(mock))
      setLoading(false)
      return
    }
    // B4: retry with backoff, then fall back to the last-good cached list so
    // a transient IPC/backend hiccup can't blank the integrations page.
    const attempts = [0, 1000, 3000]
    let lastErr: unknown = null
    for (const delay of attempts) {
      if (delay > 0) await new Promise((r) => setTimeout(r, delay))
      try {
        const data = await invoke<IntegrationStatus[]>('get_integrations')
        setIntegrations(toFrontend(data))
        if (shouldClearError(true, !!opts?.preserveError)) setError(null)
        lastErr = null
        try {
          localStorage.setItem('fem.integrations.lastGood', JSON.stringify(data))
        } catch { /* storage unavailable — cache skipped */ }
        break
      } catch (e) {
        lastErr = e
      }
    }
    if (lastErr !== null) {
      console.warn('get_integrations failed after retries:', lastErr)
      setError(String(lastErr))
      setIntegrations((prev) => {
        if (prev.length > 0) return prev
        try {
          const cached = localStorage.getItem('fem.integrations.lastGood')
          if (cached) return toFrontend(JSON.parse(cached) as IntegrationStatus[])
        } catch { /* ignore bad cache */ }
        return prev
      })
    }
    setLoading(false)
  }, [])

  const fetchSystem = useCallback(async () => {
    if (!isTauri()) {
      // Browser preview/e2e hint (mirrors useDevice's ?wizard=1 pattern):
      // ?docker=<kind> simulates a system status without IPC.
      const hint = new URLSearchParams(window.location.search).get('docker')
      if (hint) {
        setSystem({
          docker: hint as SystemStatus['docker'],
          // G4 review finding 15: an empty string here is falsy, so
          // Integrations.tsx's dockerNote (and therefore dockerBlocked)
          // could never be exercised under this hint even when `docker`
          // itself was a non-ready kind — the not-ready badge state was
          // structurally unreachable from browser preview. A real message
          // makes the hint actually simulate what it claims to.
          docker_message: `Docker ${hint.replace(/_/g, ' ')}`,
          virtualization_supported: true
        })
      }
      return
    }
    try {
      const status = await invoke<SystemStatus>('get_system_status')
      setSystem(status)
    } catch (e) {
      console.warn('get_system_status failed:', e)
    }
  }, [])

  useEffect(() => {
    fetch()
    fetchSystem()
  }, [fetch, fetchSystem])

  // Consent state for an integration that tracks one. Never throws: a status
  // we cannot read is reported as unknown so the caller can decide.
  // (Moved above the poll effect below, which — B18 D3 — now also calls
  // this on every tick, not just on mount.)
  const refreshConsent = useCallback(async (id: string): Promise<ConsentStatus | null> => {
    if (!isTauri()) return null
    try {
      const status = await invoke<ConsentStatus>('check_consent', { integrationId: id })
      setConsentActive((prev) => ({ ...prev, [id]: status.active }))
      return status
    } catch (e) {
      console.warn(`check_consent(${id}) failed:`, e)
      return null
    }
  }, [])

  // Poll as a fallback so the UI can never go permanently stale if the
  // health-event stream dies (and to pick up Docker state changes).
  // B18 D3: also re-check consent here — it was previously fetched only on
  // mount, so a consent loss between polls (e.g. a supervisor restart
  // wrongly recording a withdrawal, B18 D1/D2) left the badge stuck on
  // "Consent active" until the user next navigated away and back.
  useEffect(() => {
    const timer = setInterval(() => {
      fetch()
      fetchSystem()
      INTEGRATION_META.filter((m) => requiresConsent(m.id)).forEach((m) => {
        refreshConsent(m.id)
      })
    }, 30_000)
    return () => clearInterval(timer)
  }, [fetch, fetchSystem, refreshConsent])

  // Listen to real-time health events emitted by the backend health loop.
  useEffect(() => {
    if (!isTauri()) return
    let unlisten: (() => void) | undefined
    const setup = async () => {
      unlisten = await listen<{ integration_id: string; status: HealthStatus; restart_count: number }>(
        'health-event',
        (event) => {
          const { integration_id: id, status: health } = event.payload
          setIntegrations((prev) =>
            prev.map((i) => {
              if (i.id !== id) return i
              const enabled = i.enabled
              return {
                ...i,
                health,
                healthy: health === 'Healthy',
                lifecycle: deriveLifecycle(enabled, health)
              }
            })
          )
        }
      )
    }
    setup()
    return () => {
      unlisten?.()
    }
  }, [])

  // Docker preflight progress (download/install/engine wait) from the backend.
  useEffect(() => {
    if (!isTauri()) return
    let unlisten: (() => void) | undefined
    const setup = async () => {
      unlisten = await listen<DockerProgress>('docker-progress', (event) => {
        if (event.payload.stage === 'ready') {
          setDockerProgress(null)
        } else {
          setDockerProgress(event.payload)
        }
      })
    }
    setup()
    return () => {
      unlisten?.()
    }
  }, [])

  useEffect(() => {
    INTEGRATION_META.filter((m) => requiresConsent(m.id)).forEach((m) => {
      refreshConsent(m.id)
    })
  }, [refreshConsent])

  // The toggle itself, with no consent handling — the caller has already
  // decided this flip is allowed to happen. Returns whether the backend took it.
  const runToggle = useCallback(
    async (id: string, next: boolean): Promise<boolean> => {
      const current = integrations.find((i) => i.id === id)

      // Optimistically flip state and show Installing while the backend installs/starts.
      setIntegrations((prev) =>
        prev.map((i) =>
          i.id === id
            ? {
                ...i,
                enabled: next,
                lifecycle: next ? 'Installing' : 'Disabled',
                healthy: next ? i.healthy : i.healthy
              }
            : i
        )
      )

      inflightToggles.current += 1
      let ok = false
      try {
        await invoke('toggle_integration', { id, enabled: next })
        setError(null)
        ok = true
      } catch (e) {
        console.warn(`toggle_integration(${id}, ${next}) failed:`, e)
        if (isConsentRequiredError(e)) {
          // The backend refused for want of consent (e.g. it was withdrawn in
          // another window). Ask for it rather than showing the sentinel.
          const status = await refreshConsent(id)
          if (status) {
            setConsentPrompt(status)
            setError(null)
          } else {
            setError(`${current?.name ?? id}: bandwidth-sharing consent could not be confirmed.`)
          }
        } else {
          setError(`${current?.name ?? id}: ${extractErrorMessage(e)}`)
        }
      } finally {
        inflightToggles.current -= 1
        if (inflightToggles.current === 0) {
          setDockerProgress(null)
        }
        // Resync with backend truth (success AND failure) so the toggle can
        // never display a state the backend doesn't hold — but keep the
        // error just set above alive instead of letting this call's own
        // success path erase it (B14 D3).
        await fetch({ preserveError: true })
        fetchSystem()
      }
      return ok
    },
    [integrations, fetch, fetchSystem, refreshConsent]
  )

  const toggle = useCallback(
    async (id: string) => {
      const current = integrations.find((i) => i.id === id)
      if (!current) return
      const next = !current.enabled

      if (requiresConsent(id)) {
        if (next) {
          // Ask before enabling, so the consent exists before anything starts.
          const status = await refreshConsent(id)
          if (shouldOpenDialog(status)) {
            if (!status) {
              setError(`${current.name}: bandwidth-sharing consent could not be confirmed.`)
              return
            }
            setConsentPrompt(status)
            return
          }
          await runToggle(id, true)
          return
        }

        // Disabling: the integration's own stop() writes the withdrawal, so a
        // successful toggle already recorded it. Only record one here when the
        // toggle failed before stop() could run, which would otherwise leave
        // the log claiming consent the user has just taken back.
        const ok = await runToggle(id, false)
        if (!ok) {
          try {
            await invoke('revoke_consent', { integrationId: id })
          } catch (e) {
            console.warn(`revoke_consent(${id}) failed:`, e)
          }
        }
        await refreshConsent(id)
        return
      }

      await runToggle(id, next)
    },
    [integrations, refreshConsent, runToggle]
  )

  const confirmConsent = useCallback(
    async (checked: boolean) => {
      const status = consentPrompt
      const action = nextActionAfterConfirm(status, checked)
      if (action.kind === 'blocked' || !status) return

      setConsentBusy(true)
      try {
        await invoke('grant_consent', {
          integrationId: status.integration_id,
          wordingVersion: action.wordingVersion
        })
      } catch (e) {
        console.warn(`grant_consent(${status.integration_id}) failed:`, e)
        setError(`Pawns.app: ${extractErrorMessage(e)}`)
        setConsentBusy(false)
        return
      }
      setConsentPrompt(null)
      setConsentBusy(false)
      await refreshConsent(status.integration_id)
      await runToggle(status.integration_id, true)
    },
    [consentPrompt, refreshConsent, runToggle]
  )

  // Declining changes nothing: no record is written and the toggle stays off.
  const cancelConsent = useCallback(() => setConsentPrompt(null), [])

  // F2: clean-slate reinstall for a stuck integration (backend wipes all
  // install artifacts, then installs + starts). Currently Olostep-only.
  const forceReinstall = useCallback(
    async (id: string) => {
      const current = integrations.find((i) => i.id === id)
      setIntegrations((prev) =>
        prev.map((i) => (i.id === id ? { ...i, lifecycle: 'Installing' } : i))
      )
      inflightToggles.current += 1
      try {
        await invoke('force_reinstall_integration', { id })
        setError(null)
      } catch (e) {
        console.warn(`force_reinstall_integration(${id}) failed:`, e)
        setError(`${current?.name ?? id}: ${extractErrorMessage(e)}`)
      } finally {
        inflightToggles.current -= 1
        if (inflightToggles.current === 0) {
          setDockerProgress(null)
        }
        // Same as runToggle: preserve the error this call just set (B14 D3).
        await fetch({ preserveError: true })
        fetchSystem()
      }
    },
    [integrations, fetch, fetchSystem]
  )

  return {
    integrations,
    loading,
    error,
    toggle,
    forceReinstall,
    refetch: fetch,
    system,
    dockerProgress,
    consentPrompt,
    consentBusy,
    consentActive,
    confirmConsent,
    cancelConsent
  }
}
