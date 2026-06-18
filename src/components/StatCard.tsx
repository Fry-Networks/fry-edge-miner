import type { LucideIcon } from 'lucide-react'
import Lbl from './primitives/Lbl'

interface StatCardProps {
  Icon: LucideIcon
  label: string
  value: string
  sub?: string
  accent?: string
}

export default function StatCard({ Icon, label, value, sub, accent = 'var(--teal)' }: StatCardProps) {
  return (
    <div
      style={{
        background: 'var(--s1)',
        border: '1px solid var(--b0)',
        borderRadius: 'var(--rad)',
        padding: '16px 18px',
        flex: 1,
        minWidth: 155,
        borderTop: `3px solid ${accent}`
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 7, marginBottom: 9 }}>
        <Icon size={13} color={accent} />
        <Lbl>{label}</Lbl>
      </div>
      <div
        style={{
          fontFamily: 'var(--fm)',
          fontSize: 26,
          fontWeight: 500,
          color: 'var(--txt)',
          lineHeight: 1
        }}
      >
        {value}
      </div>
      {sub && <div style={{ fontFamily: 'var(--fb)', fontSize: 11, color: 'var(--t2)', marginTop: 5 }}>{sub}</div>}
    </div>
  )
}
