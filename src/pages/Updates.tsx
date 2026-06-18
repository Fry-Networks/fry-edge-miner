import { useState } from 'react'
import {
  ArrowUpCircle,
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
import Btn from '../components/primitives/Btn'
import Lbl from '../components/primitives/Lbl'
import UpdCard, { type UpdateStatus } from '../components/UpdCard'

const PARTNERS: {
  key: string
  name: string
  Icon: LucideIcon
  col: string
  current: string
  available: string
  initial: UpdateStatus
}[] = [
  {
    key: 'myst',
    name: 'Mysterium (MystNodes SDK)',
    Icon: Globe,
    col: '#4a9eff',
    current: '1.2.3',
    available: '1.3.0',
    initial: 'update'
  },
  {
    key: 'pre',
    name: 'Presearch Node',
    Icon: Search,
    col: '#a855f7',
    current: '0.9.1',
    available: '0.9.1',
    initial: 'ok'
  },
  {
    key: 'dii',
    name: 'Diiisco',
    Icon: Cpu,
    col: '#f0a500',
    current: '2.0.1',
    available: '2.0.1',
    initial: 'ok'
  },
  {
    key: 'space',
    name: 'SpaceAcres',
    Icon: HardDrive,
    col: '#22c55e',
    current: '2.1.0',
    available: '2.1.0',
    initial: 'ok'
  },
  {
    key: 'olo',
    name: 'Olostep',
    Icon: Eye,
    col: '#00c49a',
    current: '1.0.0',
    available: '1.0.0',
    initial: 'ok'
  }
]

export default function Updates() {
  const [checking, setChecking] = useState(false)
  const [updStatus, setUpdStatus] = useState<Record<string, UpdateStatus>>({})

  const doUpdate = (key: string) => {
    setUpdStatus((p) => ({ ...p, [key]: 'updating' }))
    setTimeout(() => setUpdStatus((p) => ({ ...p, [key]: 'ok' })), 2200)
  }

  const s = (key: string): UpdateStatus => updStatus[key] || PARTNERS.find((p) => p.key === key)?.initial || 'ok'

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
          <span style={{ fontFamily: 'var(--fb)', fontSize: 12, color: 'var(--t1)' }}>Last checked 2 minutes ago</span>
        </div>
        <Btn
          v="g"
          onClick={() => {
            setChecking(true)
            setTimeout(() => setChecking(false), 1600)
          }}
          sx={{ padding: '5px 12px', fontSize: 11 }}
        >
          <RefreshCw size={11} style={{ animation: checking ? 'spin 1s linear infinite' : 'none' }} />
          {checking ? 'Checking…' : 'Check for updates'}
        </Btn>
      </div>

      <div>
        <Lbl sx={{ marginBottom: 9 }}>Application</Lbl>
        <UpdCard
          name="Fry Edge Miner"
          Icon={ArrowUpCircle}
          col="var(--red)"
          current="0.2.3"
          available="0.2.3"
          status={s('fem') || 'ok'}
        />
      </div>

      <div>
        <Lbl sx={{ marginBottom: 9 }}>Partner Software</Lbl>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 7 }}>
          {PARTNERS.map((p) => (
            <UpdCard
              key={p.key}
              name={p.name}
              Icon={p.Icon}
              col={p.col}
              current={p.current}
              available={p.available}
              status={s(p.key)}
              onUpdate={() => doUpdate(p.key)}
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
