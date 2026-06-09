import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { FemConfig } from '../lib/types'

export function useSettings() {
  const [config, setConfig] = useState<FemConfig | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const fetch = useCallback(async () => {
    try {
      const data = await invoke<FemConfig>('get_config')
      setConfig(data)
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

  const save = useCallback(
    async (newConfig: Partial<FemConfig>) => {
      try {
        await invoke('save_config', { config: newConfig })
        await fetch() // re-fetch after save
      } catch (e) {
        setError(String(e))
      }
    },
    [fetch]
  )

  return { config, loading, error, save, refetch: fetch }
}
