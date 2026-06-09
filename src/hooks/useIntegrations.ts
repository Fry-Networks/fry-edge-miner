import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { IntegrationStatus } from '../lib/types'

export function useIntegrations() {
  const [integrations, setIntegrations] = useState<IntegrationStatus[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const fetch = useCallback(async () => {
    try {
      const data = await invoke<IntegrationStatus[]>('get_integrations')
      setIntegrations(data)
      setError(null)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    fetch()
  }, [fetch])

  const toggle = useCallback(
    async (id: string, enabled: boolean) => {
      try {
        await invoke('toggle_integration', { id, enabled })
        await fetch() // re-fetch after toggle
      } catch (e) {
        setError(String(e))
      }
    },
    [fetch]
  )

  return { integrations, loading, error, toggle, refetch: fetch }
}
