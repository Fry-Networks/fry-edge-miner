import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { RewardSummary, PocSlot } from '../lib/types'

export interface RewardsData {
  summary: RewardSummary
  slots: PocSlot[]
}

export function useRewards() {
  const [rewards, setRewards] = useState<RewardsData | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const fetch = useCallback(async () => {
    try {
      const summary = await invoke<RewardSummary>('get_reward_summary')
      const slots = await invoke<PocSlot[]>('get_poc_slots')
      setRewards({ summary, slots })
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

  return { rewards, loading, error, refetch: fetch }
}
