import type { ReactNode } from 'react'

interface StatCardProps {
  label: string
  value: string | number
  subtitle?: string
  icon: ReactNode
  accent?: string
}

export function StatCard({ label, value, subtitle, icon, accent = '#00B69B' }: StatCardProps) {
  return (
    <div className="relative bg-fry-surface border border-fry-border rounded-xl p-6 overflow-hidden">
      <div className="absolute top-0 left-0 right-0 h-[3px] rounded-t-xl" style={{ background: accent }} />
      <div className="absolute top-4 right-4 text-fry-text-muted opacity-25">{icon}</div>
      <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted mt-4">{label}</p>
      <p className="text-3xl font-bold tabular-nums text-fry-text mt-1">{value}</p>
      {subtitle && <p className="text-xs text-fry-text-muted mt-1">{subtitle}</p>}
    </div>
  )
}
