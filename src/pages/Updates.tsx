import { useCallback, useEffect, useState } from 'react'
import {
  ArrowUpCircle,
  CheckCircle2,
  Clock,
  Cpu,
  Eye,
  Globe,
  HardDrive,
  Info,
  RefreshCw,
  Search,
  type LucideIcon
} from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import Btn from '../components/primitives/Btn'
import Lbl from '../components/primitives/Lbl'
import UpdCard, { type UpdateStatus } from '../components/UpdCard'
import type { UpdateInfo } from '../lib/types'
import { APP_VERSION } from '../lib/version'

const ICONS: Record<string, LucideIcon> = {
  fem: ArrowUpCircle,
  mysterium: Globe,
  presearch: Search,
  diiisco: Cpu,
  space_acres: HardDrive,
  aem: Eye
}

const COLORS: Record<string, string> = {
  fem: 'var(--red)',
  mysterium: '#4a9eff',
  presearch: '#a855f7',
  diiisco: '#f0a500',
  space_acres: '#22c55e',
  aem: '#00c49a'
}

const NAMES: Record<string, string> = {
  mysterium: 'Mysterium (MystNodes SDK)',
  presearch: 'Presearch Node',
  diiisco: 'Diiisco',
  space_acres: 'SpaceAcres',
  aem: 'Olostep'
}

function isTauri(): boolean {
  return typeof window !== 'undefined' && !!(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__
}

function mockUpdates(): UpdateInfo[] {
  return [
    {
      id: 'fem',
      name: 'Fry Edge Miner',
      current_version: APP_VERSION,
      latest_version: APP_VERSION,
      available: false,
      kind: 'app',
      download_url: null,
      body: null
    },
    {
      id: 'mysterium',
      name: 'Mysterium (MystNodes SDK)',
      current_version: '1.2.3',
      latest_version: '1.3.0',
      available: true,
      kind: 'integration',
      download_url: null,
      body: null
    },
    {
      id: 'presearch',
      name: 'Presearch Node',
      current_version: '0.9.1',
      latest_version: '0.9.1',
      available: false,
      kind: 'integration',
      download_url: null,
      body: null
    },
    {
      id: 'diiisco',
      name: 'Diiisco',
      current_version: '2.0.1',
      latest_version: '2.0.1',
      available: false,
      kind: 'integration',
      download_url: null,
      body: null
    },
    {
      id: 'space_acres',
      name: 'SpaceAcres',
      current_version: '2.1.0',
      latest_version: '2.1.0',
      available: false,
      kind: 'integration',
      download_url: null,
      body: null
    },
    {
      id: 'aem',
      name: 'Olostep',
      current_version: '1.0.0',
      latest_version: '1.0.0',
      available: false,
      kind: 'integration',
      download_url: null,
      body: null
    }
  ]
}

interface DisplayUpdate extends UpdateInfo {
  key: string
  Icon: LucideIcon
  col: string
}

function toDisplay(updates: UpdateInfo[]): DisplayUpdate[] {
  return updates.map((u) => ({
    ...u,
    key: u.id,
    Icon: ICONS[u.id] || Eye,
    col: COLORS[u.id] || '#00c49a'
  }))
}

function formatLastChecked(d: Date): string {
  const seconds = Math.floor((Date.now() - d.getTime()) / 1000)
  if (seconds < 60) return 'just now'
  if (seconds < 3600) return `${Math.floor(seconds / 60)} minute${seconds < 120 ? '' : 's'} ago`
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} hour${seconds < 7200 ? '' : 's'} ago`
  return d.toLocaleDateString()
}

export default function Updates() {
  const [updates, setUpdates] = useState<DisplayUpdate[]>([])
  const [checking, setChecking] = useState(false)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState<string | null>(null)
  const [lastChecked, setLastChecked] = useState<Date | null>(null)
  const [isFetching, setIsFetching] = useState(false)
  const [isUpdating, setIsUpdating] = useState(false)

  const fetchUpdates = useCallback(async () => {
    setIsFetching(true)
    setChecking(true)
    setError(null)
    setSuccess(null)
    try {
      const data = isTauri() ? await invoke<UpdateInfo[]>('check_updates') : mockUpdates()
      setUpdates(toDisplay(data))
      setLastChecked(new Date())
    } catch (e) {
      console.warn('check_updates failed:', e)
      setError(String(e))
    } finally {
      setIsFetching(false)
      setChecking(false)
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    fetchUpdates()
  }, [fetchUpdates])

  const doUpdate = useCallback(
    async (kind: string, id: string) => {
      if (isUpdating || isFetching) return
      setIsUpdating(true)
      setError(null)
      setSuccess(null)
      try {
        const result = isTauri()
          ? await invoke<string>('install_update', { kind, id })
          : `Mock update installed for ${id}`
        await fetchUpdates()
        setSuccess(result)
      } catch (e) {
        console.warn(`install_update(${kind}, ${id}) failed:`, e)
        setError(String(e))
      } finally {
        setIsUpdating(false)
      }
    },
    [fetchUpdates, isFetching, isUpdating]
  )

  const statusFor = (u: DisplayUpdate): UpdateStatus => {
    if (u.error) return 'error'
    if (u.available) return 'update'
    return 'ok'
  }

  const appUpdate = updates.find((u) => u.kind === 'app')
  const partnerUpdates = updates.filter((u) => u.kind === 'integration')

  if (loading) {
    return (
      <div
        className="sc"
        style={{
          padding: 20,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          height: '100%'
        }}
      >
        <RefreshCw size={24} style={{ animation: 'spin 1s linear infinite', color: 'var(--t2)' }} />
      </div>
    )
  }

  return (
    <div
      className="sc"
      style={{
        padding: '20px 24px',
        display: 'flex',
        flexDirection: 'column',
        gap: 14,
        overflowY: 'auto',
        height: '100%'
      }}
    >
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          padding: '11px 14px',
          background: 'var(--s2)',
          border: '1px solid var(--b0)',
          borderRadius: 'var(--rad)'
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <Clock size={12} color="var(--t2)" />
          <span style={{ fontFamily: 'var(--fb)', fontSize: 12, color: 'var(--t1)' }}>
            {lastChecked ? `Last checked ${formatLastChecked(lastChecked)}` : 'Not checked yet'}
          </span>
        </div>
        <Btn v="g" onClick={fetchUpdates} sx={{ padding: '5px 12px', fontSize: 11 }}>
          <RefreshCw
            size={11}
            style={{ animation: checking ? 'spin 1s linear infinite' : 'none' }}
          />
          {checking ? 'Checking…' : 'Check for updates'}
        </Btn>
      </div>

      {error && (
        <div
          style={{
            padding: '10px 14px',
            background: 'var(--red)12',
            border: '1px solid var(--red)40',
            borderRadius: 'var(--rad)',
            fontFamily: 'var(--fb)',
            fontSize: 12,
            color: 'var(--red)'
          }}
        >
          {error}
        </div>
      )}

      {success && (
        <div
          style={{
            padding: '10px 14px',
            background: 'var(--teal)12',
            border: '1px solid var(--teal)40',
            borderRadius: 'var(--rad)',
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            fontFamily: 'var(--fb)',
            fontSize: 12,
            color: 'var(--teal)'
          }}
        >
          <CheckCircle2 size={13} />
          {success}
        </div>
      )}

      <div>
        <Lbl sx={{ marginBottom: 9 }}>Application</Lbl>
        {appUpdate ? (
          <UpdCard
            name={appUpdate.name}
            Icon={appUpdate.Icon}
            col={appUpdate.col}
            current={appUpdate.current_version || undefined}
            available={appUpdate.latest_version || undefined}
            status={statusFor(appUpdate)}
            onUpdate={appUpdate.available ? () => doUpdate('app', appUpdate.id) : undefined}
          />
        ) : (
          <UpdCard
            name="Fry Edge Miner"
            Icon={ArrowUpCircle}
            col="var(--red)"
            current={undefined}
            available={undefined}
            status="ok"
          />
        )}
      </div>

      <div>
        <Lbl sx={{ marginBottom: 9 }}>Partner Software</Lbl>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 7 }}>
          {partnerUpdates.map((p) => (
            <UpdCard
              key={p.id}
              name={NAMES[p.id] || p.name}
              Icon={p.Icon}
              col={p.col}
              current={p.current_version || undefined}
              available={p.latest_version || undefined}
              status={statusFor(p)}
              onUpdate={p.available ? () => doUpdate('integration', p.id) : undefined}
            />
          ))}
        </div>
      </div>

      <div
        style={{
          background: 'var(--s2)',
          border: '1px solid var(--b0)',
          borderRadius: 'var(--rad)',
          padding: '12px 14px',
          display: 'flex',
          gap: 9
        }}
      >
        <Info size={13} color="var(--t2)" style={{ marginTop: 1, flexShrink: 0 }} />
        <div
          style={{
            fontFamily: 'var(--fb)',
            fontSize: 12,
            color: 'var(--t1)',
            lineHeight: 1.6
          }}
        >
          FEM checks partner software versions via hardwareapi on startup and every 6 hours.
          Application updates use Tauri&apos;s built-in updater with ed25519 signature verification.
        </div>
      </div>
    </div>
  )
}
