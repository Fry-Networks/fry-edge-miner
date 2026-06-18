import type { CSSProperties, ReactNode } from 'react'

interface SettingRowProps {
  label: string
  sub?: string
  children?: ReactNode
  sx?: CSSProperties
}

export default function SettingRow({ label, sub, children, sx }: SettingRowProps) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        padding: '12px 0',
        borderBottom: '1px solid var(--b0)',
        ...sx
      }}
    >
      <div>
        <div style={{ fontFamily: 'var(--fb)', fontWeight: 500, fontSize: 13, color: 'var(--txt)' }}>{label}</div>
        {sub && <div style={{ fontFamily: 'var(--fb)', fontSize: 11, color: 'var(--t2)', marginTop: 1 }}>{sub}</div>}
      </div>
      {children}
    </div>
  )
}
