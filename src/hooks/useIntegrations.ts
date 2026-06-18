import { useCallback, useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { IntegrationStatus } from '../lib/types'
import { INTGS, type MockIntegration } from '../lib/data'

const TAGS: Record<string, string> = {
  mysterium: 'VPN NODE',
  presearch: 'SEARCH NODE',
  diiisco: 'AI NODE',
  spacetwo: 'STORAGE NODE',
  olostep: 'SCRAPE NODE'
}

function toMock(integrations: IntegrationStatus[]): MockIntegration[] {
  return integrations.map((i) => {
    const base = INTGS.find((m) => m.id === i.id)
    return {
      id: i.id,
      name: i.display_name || base?.name || i.id,
      tag: TAGS[i.id] || base?.tag || 'NODE',
      desc: base?.desc || '',
      Icon: base?.Icon || INTGS[0].Icon,
      col: base?.col || '#00c49a',
      enabled: i.enabled,
      healthy: i.health === 'Healthy',
      version: i.version,
      uptime: base?.uptime ?? 0
    }
  })
}

export function useIntegrations() {
  const [integrations, setIntegrations] = useState<MockIntegration[]>(INTGS)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const fetch = useCallback(async () => {
    try {
      const data = await invoke<IntegrationStatus[]>('get_integrations')
      setIntegrations(toMock(data))
      setError(null)
    } catch (e) {
      console.warn('get_integrations failed, using mock fallback:', e)
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    fetch()
  }, [fetch])

  const toggle = useCallback(
    async (id: string) => {
      const current = integrations.find((i) => i.id === id)
      if (!current) return
      const next = !current.enabled
      try {
        await invoke('toggle_integration', { id, enabled: next })
        setIntegrations((prev) =>
          prev.map((i) =>
            i.id === id ? { ...i, enabled: next, healthy: next ? true : i.healthy } : i
          )
        )
        setError(null)
      } catch (e) {
        console.warn(`toggle_integration(${id}, ${next}) failed:`, e)
        setError(String(e))
      }
    },
    [integrations]
  )

  return { integrations, loading, error, toggle, refetch: fetch }
}
