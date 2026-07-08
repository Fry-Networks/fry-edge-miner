import { Download } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import Btn from './primitives/Btn'
import Tag from './primitives/Tag'

export type UpdateStatus = 'ok' | 'updating' | 'update' | 'error'

interface UpdCardProps {
  name: string
  Icon: LucideIcon
  col: string
  current?: string
  available?: string
  status: UpdateStatus
  onUpdate?: () => void
}

export default function UpdCard({ name, Icon, col, current, available, status, onUpdate }: UpdCardProps) {
  const hasUpd = status === 'update'
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 12,
        padding: '13px 16px',
        background: 'var(--s1)',
        border: '1px solid var(--b0)',
        borderRadius: 'var(--rad)',
        borderLeft: `3px solid ${
          hasUpd ? 'var(--amb)' : status === 'updating' ? 'var(--blu)' : status === 'error' ? 'var(--amb)' : 'var(--teal)'
        }`
      }}
    >
      <div
        style={{
          width: 36,
          height: 36,
          borderRadius: 'var(--radsm)',
          flexShrink: 0,
          background: `${col}12`,
          border: `1px solid ${col}24`,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center'
        }}
      >
        <Icon size={16} color={col} style={status === 'updating' ? { animation: 'spin 1s linear infinite' } : {}} />
      </div>
      <div style={{ flex: 1 }}>
        <div style={{ fontFamily: 'var(--fh)', fontWeight: 700, fontSize: 13, color: 'var(--txt)', marginBottom: 2 }}>{name}</div>
        <div style={{ fontFamily: 'var(--fm)', fontSize: 10, color: 'var(--t2)' }}>
          {current ? (/^\d/.test(current) ? `v${current}` : 'Installed') : 'Not installed'}
          {hasUpd && (
            <>
              {' '}→ <span style={{ color: 'var(--amb)' }}>v{available}</span>
            </>
          )}
          {status === 'updating' && <span style={{ color: 'var(--blu)' }}> — downloading…</span>}
        </div>
      </div>
      {status === 'ok' && <Tag v="run">Up to date</Tag>}
      {status === 'error' && <Tag v="warn">Check failed</Tag>}
      {status === 'updating' && <Tag v="info">Updating…</Tag>}
      {status === 'update' && (
        <Btn v="p" sx={{ padding: '4px 12px', fontSize: 11 }} onClick={onUpdate}>
          <Download size={11} /> Update
        </Btn>
      )}
    </div>
  )
}
