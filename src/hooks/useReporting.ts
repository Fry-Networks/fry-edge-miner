import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { isTauri } from '../lib/tauri'

// Live PoC/lease reporting health from the backend loop (B2) — the UI must
// show truthful reporting state instead of silently going stale.
export interface ReportingStatus {
  registered: boolean
  last_tick_at: string | null
  last_poc_ok_at: string | null
  last_poc_error: string | null
  consecutive_poc_failures: number
  lease_active: boolean
  lease_error: string | null
}

export function useReporting() {
  const [status, setStatus] = useState<ReportingStatus | null>(null)

  useEffect(() => {
    if (!isTauri()) return
    let cancelled = false
    let unlisten: (() => void) | undefined

    invoke<ReportingStatus>('get_reporting_status')
      .then((s) => {
        if (!cancelled) setStatus(s)
      })
      .catch(() => {
        /* backend not ready yet — the event stream will update us */
      })

    listen<ReportingStatus>('reporting-status', (ev) => {
      setStatus(ev.payload)
    }).then((u) => {
      if (cancelled) u()
      else unlisten = u
    })

    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  return status
}
