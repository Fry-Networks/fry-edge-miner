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

export interface HourlyGates {
  data: boolean
  online: boolean
  mac_match: boolean
  pol: boolean
  poi: boolean
  poa: boolean
}

function toPocSlots(slots: BackendPocSlot[]): PocSlotUi[] {
  if (!slots.length) return []
  return slots.map((s) => ({ done: s.online || s.data, pass: s.online || s.data }))
}

// 144 slots/day ÷ 24h — mirrors SLOT_INTERVAL_MINUTES=10 in poc/reporter.rs.
const SLOTS_PER_HOUR = 6

function toHourlyGates(slots: BackendPocSlot[]): HourlyGates[] {
  const hours: HourlyGates[] = Array.from({ length: 24 }, () => ({
    data: false, online: false, mac_match: false,
    pol: false, poi: false, poa: false
  }))
  for (const s of slots) {
    const h = Math.floor(s.slot_index / SLOTS_PER_HOUR)
    if (h < 24) {
      hours[h].data = hours[h].data || s.data
      hours[h].online = hours[h].online || s.online
      hours[h].mac_match = hours[h].mac_match || s.mac_match
      hours[h].pol = hours[h].pol || s.pol
      hours[h].poi = hours[h].poi || s.poi
      hours[h].poa = hours[h].poa || s.poa
    }
  }
  return hours
}

export interface RewardsData {
  summary: RewardSummary | null
  rows: RewardRow[]
  slots: PocSlotUi[]
  hourlyGates: HourlyGates[]
}

export function useRewards() {
  const [rewards, setRewards] = useState<RewardsData>({
    summary: null,
    rows: [],
    slots: [],
    hourlyGates: []
  })
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const fetch = useCallback(async () => {
    try {
      const summary = await invoke<RewardSummary>('get_reward_summary')
      const slots = await invoke<BackendPocSlot[]>('get_poc_slots')
      setRewards({ summary, rows: toRewardRows(summary), slots: toPocSlots(slots), hourlyGates: toHourlyGates(slots) })
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
