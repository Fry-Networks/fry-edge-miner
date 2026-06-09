import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { DeviceInfo } from '../lib/types'

export function useDevice() {
  const [device, setDevice] = useState<DeviceInfo | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const fetch = useCallback(async () => {
    try {
      const data = await invoke<DeviceInfo>('get_device_info')
      setDevice(data)
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

  const setWallet = useCallback(
    async (address: string) => {
      try {
        await invoke('set_wallet_address', { address })
        await fetch() // re-fetch after change
      } catch (e) {
        setError(String(e))
      }
    },
    [fetch]
  )

  return { device, loading, error, setWallet, refetch: fetch }
}
