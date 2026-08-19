import type { LucideIcon } from 'lucide-react'
import Lbl from './primitives/Lbl'

interface StatCardProps {
  Icon: LucideIcon
  label: string
  value: string
  sub?: string
  /** Secondary line under `sub`, for a count that is a bonus rather than part
   *  of the headline figure (F2: community/SDK integrations). */
  sub2?: string
  accent?: string
}

export default function StatCard({ Icon, label, value, sub, sub2, accent = 'var(--teal)' }: StatCardProps) {
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
      {sub2 && <div style={{ fontFamily: 'var(--fb)', fontSize: 11, color: 'var(--amb)', marginTop: 2 }}>{sub2}</div>}
    </div>
  )
}
