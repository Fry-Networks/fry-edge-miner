import { useCallback, useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { DeviceInfo } from '../lib/types'

const FALLBACK: DeviceInfo = {
  miner_key: 'FEM-b9e489c8a32d5547bbb7c363baaf733e',
  wallet_address: 'OGHVJYWQXOOPZG2OLBIRFNTBF3H3276DDTKYYZUA6G4NUMF2RGYXNTMIRE',
  registered: true
}

export function useDevice() {
  const [device, setDevice] = useState<DeviceInfo | null>(FALLBACK)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const fetch = useCallback(async () => {
    try {
      const data = await invoke<DeviceInfo>('get_device_info')
      setDevice(data)
      setError(null)
    } catch (e) {
      console.warn('get_device_info failed, using mock fallback:', e)
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    fetch()
  }, [fetch])

  const register = useCallback(
    async (wallet: string) => {
      try {
        const minerKey = await invoke<string>('register_device', { wallet })
        await fetch()
        return minerKey
      } catch (e) {
        setError(String(e))
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
      setError(String(e))
      throw e
    }
  }, [fetch])

  return { device, loading, error, register, deregister, refetch: fetch }
}
