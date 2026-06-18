import { useCallback, useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { RewardSummary } from '../lib/types'
import { POC, RWDS, type PocSlot, type RewardRow } from '../lib/data'

function toRewardRows(summary: RewardSummary | null): RewardRow[] {
  if (!summary) return RWDS
  return [
    {
      date: 'Latest',
      reward: summary.estimated_daily,
      slots: Math.round(summary.proportion * 144),
      factor: summary.proportion,
      status: 'paid'
    },
    ...RWDS.slice(1)
  ]
}

function toPocSlots(slots: { data?: boolean; online?: boolean }[]): PocSlot[] {
  if (!slots.length) return POC
  return slots.map((s) => ({ done: s.online ?? s.data ?? false, pass: s.online ?? s.data ?? false }))
}

export interface RewardsData {
  summary: RewardSummary | null
  rows: RewardRow[]
  slots: PocSlot[]
}

export function useRewards() {
  const [rewards, setRewards] = useState<RewardsData>({
    summary: null,
    rows: RWDS,
    slots: POC
  })
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const fetch = useCallback(async () => {
    try {
      const summary = await invoke<RewardSummary>('get_reward_summary')
      const slots = await invoke<{ data?: boolean; online?: boolean }[]>('get_poc_slots')
      setRewards({ summary, rows: toRewardRows(summary), slots: toPocSlots(slots) })
      setError(null)
    } catch (e) {
      console.warn('rewards fetch failed, using mock fallback:', e)
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
