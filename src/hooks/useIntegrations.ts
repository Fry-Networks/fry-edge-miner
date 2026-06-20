import { useCallback, useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { HealthStatus, IntegrationStatus, LifecycleState } from '../lib/types'
import { INTEGRATION_META, type IntegrationMeta } from '../lib/integrationMeta'

function isTauri(): boolean {
  return typeof window !== 'undefined' && !!(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__
}

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
}

function toMock(integrations: IntegrationStatus[]): FrontendIntegration[] {
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
      poc_contribution: 1 / integrations.length
    }
  })
}

export function useIntegrations() {
  const [integrations, setIntegrations] = useState<FrontendIntegration[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const fetch = useCallback(async () => {
    try {
      const data = await invoke<IntegrationStatus[]>('get_integrations')
      setIntegrations(toMock(data))
      setError(null)
    } catch (e) {
      console.warn('get_integrations failed:', e)
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    fetch()
  }, [fetch])

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
                healthy: next ? true : i.healthy
              }
            : i
        )
      )

      try {
        await invoke('toggle_integration', { id, enabled: next })
        setError(null)
        await fetch()
      } catch (e) {
        console.warn(`toggle_integration(${id}, ${next}) failed:`, e)
        // Revert to the previous known state.
        setIntegrations((prev) =>
          prev.map((i) =>
            i.id === id
              ? {
                  ...i,
                  enabled: current.enabled,
                  healthy: current.healthy,
                  health: current.health,
                  lifecycle: current.lifecycle
                }
              : i
          )
        )
        setError(String(e))
      }
    },
    [integrations, fetch]
  )

  return { integrations, loading, error, toggle, refetch: fetch }
}
