import { useCallback, useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type {
  RewardSummary,
  PocSlot as BackendPocSlot,
  RewardRow,
  PocSlotUi
} from '../lib/types'

function toRewardRows(summary: RewardSummary | null): RewardRow[] {
  if (!summary) return []
  return [
    {
      date: 'Latest',
      reward: summary.estimated_daily,
      slots: Math.round(summary.proportion * 144),
      factor: summary.proportion,
      status: 'paid'
    }
  ]
}

function toPocSlots(slots: BackendPocSlot[]): PocSlotUi[] {
  if (!slots.length) return []
  return slots.map((s) => ({ done: s.online || s.data, pass: s.online || s.data }))
}

export interface RewardsData {
  summary: RewardSummary | null
  rows: RewardRow[]
  slots: PocSlotUi[]
}

export function useRewards() {
  const [rewards, setRewards] = useState<RewardsData>({
    summary: null,
    rows: [],
    slots: []
  })
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const fetch = useCallback(async () => {
    try {
      const summary = await invoke<RewardSummary>('get_reward_summary')
      const slots = await invoke<BackendPocSlot[]>('get_poc_slots')
      setRewards({ summary, rows: toRewardRows(summary), slots: toPocSlots(slots) })
      setError(null)
    } catch (e) {
      console.warn('rewards fetch failed:', e)
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
