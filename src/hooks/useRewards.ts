import { useCallback, useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type {
  RewardSummary,
  PocSlot as BackendPocSlot,
  RewardRow,
  PocSlotUi
} from '../lib/types'
import { isSummaryReady } from '../lib/rewardReadiness'

export { isSummaryReady }

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

// While a not-ready summary is showing, re-poll every 5s until it resolves —
// bounded so a permanently-unregistered device (which never becomes "ready"
// via config alone) doesn't poll forever.
export const NOT_READY_POLL_MS = 5000
export const NOT_READY_POLL_CAP = 60

/**
 * Pure polling decision, unit-testable without mounting the hook.
 * `hasSummary` — a summary has been fetched at least once (nothing to poll
 * for until the first fetch lands). `ready` — both readiness flags are true.
 * `pollsSoFar` — how many not-ready polls have already fired.
 * Returns true iff another poll should be scheduled.
 */
export function shouldPollAgain(hasSummary: boolean, ready: boolean, pollsSoFar: number): boolean {
  if (!hasSummary || ready) return false
  return pollsSoFar < NOT_READY_POLL_CAP
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
  const ready = isSummaryReady(rewards.summary)

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

  useEffect(() => {
    const hasSummary = !!rewards.summary
    if (!shouldPollAgain(hasSummary, ready, 0)) return
    let cancelled = false
    let polls = 0
    const id = setInterval(() => {
      polls += 1
      if (cancelled) return
      if (!shouldPollAgain(hasSummary, ready, polls)) {
        clearInterval(id)
        return
      }
      fetch()
    }, NOT_READY_POLL_MS)
    return () => {
      cancelled = true
      clearInterval(id)
    }
    // Re-arm whenever readiness flips (e.g. summary appears, or resolves)
    // rather than on every fetch — `fetch` itself is stable (useCallback []).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [!!rewards.summary, ready, fetch])

  return { rewards, loading, error, ready, refetch: fetch }
}
