import { useCallback, useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { DeviceInfo } from '../lib/types'
import { extractErrorMessage } from '../lib/error'
import { isTauri } from '../lib/tauri'

const BROWSER_MOCK_DEVICE: DeviceInfo = {
  miner_key: 'FEM-BROWSER-PREVIEW-MODE',
  wallet_address: 'BROWSER-PREVIEW-NO-WALLET',
  registered: true,
}

// Browser-only test hook: `?wizard=1` in URL renders wizard even when the
// mock reports the device as registered. Zero effect under real Tauri.
const BROWSER_MOCK_UNREGISTERED: DeviceInfo = {
  miner_key: '',
  wallet_address: '',
  registered: false,
}

export function useDevice() {
  const [device, setDevice] = useState<DeviceInfo | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const fetch = useCallback(async () => {
    if (!isTauri()) {
      const forceWizard =
        typeof window !== 'undefined' &&
        new URLSearchParams(window.location.search).get('wizard') === '1'
      setDevice(forceWizard ? BROWSER_MOCK_UNREGISTERED : BROWSER_MOCK_DEVICE)
      setLoading(false)
      return
    }
    try {
      const data = await invoke<DeviceInfo>('get_device_info')
      setDevice(data)
      setError(null)
    } catch (e) {
      console.warn('get_device_info failed:', e)
      setError(extractErrorMessage(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    fetch()
  }, [fetch])

  const register = useCallback(
    async (wallet: string, minerKey?: string) => {
      try {
        const result = await invoke<string>('register_device', { wallet, minerKey })
        await fetch()
        return result
      } catch (e) {
        setError(extractErrorMessage(e))
        throw e
      }
    },
    [fetch]
  )

  const deregister = useCallback(async () => {
    try {
      await invoke('deregister_device')
      await fetch()
    } catch (e) {
      setError(extractErrorMessage(e))
      throw e
    }
  }, [fetch])

  return { device, loading, error, register, deregister, refetch: fetch }
}
