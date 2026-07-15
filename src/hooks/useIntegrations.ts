import { useCallback, useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { DockerProgress, HealthStatus, IntegrationStatus, LifecycleState, SystemStatus } from '../lib/types'
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

export interface FrontendIntegration extends IntegrationMeta {
  enabled: boolean
  health: HealthStatus
  healthy: boolean
  lifecycle: LifecycleState
  version: string | null
  poc_contribution: number
  requires_docker: boolean
}

function toFrontend(integrations: IntegrationStatus[]): FrontendIntegration[] {
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
      enabled,
      health,
      healthy: health === 'Healthy',
      lifecycle,
      version: i.version,
      // Backend value is healthy-based (matches what the PoC reporter
      // actually submits); the equal-split fallback only covers browser
      // preview mode without IPC.
      poc_contribution: i.poc_contribution ?? (integrations.length > 0 ? 1 / integrations.length : 0),
      requires_docker: i.requires_docker ?? false
    }
  })
}

export function useIntegrations() {
  const [integrations, setIntegrations] = useState<FrontendIntegration[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [system, setSystem] = useState<SystemStatus | null>(null)
  const [dockerProgress, setDockerProgress] = useState<DockerProgress | null>(null)
  // Toggles currently awaiting the backend — the docker-progress banner is
  // only cleared when NO toggle is in flight, so one integration's failure
  // can't hide another's still-running Docker install.
  const inflightToggles = useRef(0)

  const fetch = useCallback(async () => {
    if (!isTauri()) {
      // Browser preview mode — show all integrations with mock data
      const mock: IntegrationStatus[] = INTEGRATION_META.map((m) => ({
        id: m.id,
        display_name: m.name,
        enabled: false,
        health: 'Stopped' as HealthStatus,
        lifecycle: 'Disabled' as LifecycleState,
        version: '0.0.0-preview',
        poc_contribution: 1 / INTEGRATION_META.length,
        requires_docker: true,
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
        setError(null)
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
    if (!isTauri()) return
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

  // Poll as a fallback so the UI can never go permanently stale if the
  // health-event stream dies (and to pick up Docker state changes).
  useEffect(() => {
    const timer = setInterval(() => {
      fetch()
      fetchSystem()
    }, 30_000)
    return () => clearInterval(timer)
  }, [fetch, fetchSystem])

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

  const toggle = useCallback(
    async (id: string) => {
      const current = integrations.find((i) => i.id === id)
      if (!current) return
      const next = !current.enabled

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
      try {
        await invoke('toggle_integration', { id, enabled: next })
        setError(null)
      } catch (e) {
        console.warn(`toggle_integration(${id}, ${next}) failed:`, e)
        setError(`${current.name}: ${extractErrorMessage(e)}`)
      } finally {
        inflightToggles.current -= 1
        if (inflightToggles.current === 0) {
          setDockerProgress(null)
        }
        // Resync with backend truth (success AND failure) so the toggle can
        // never display a state the backend doesn't hold.
        await fetch()
        fetchSystem()
      }
    },
    [integrations, fetch, fetchSystem]
  )

  return { integrations, loading, error, toggle, refetch: fetch, system, dockerProgress }
}
