import type { CSSProperties } from 'react'

interface DividerProps {
  sx?: CSSProperties
}

export default function Divider({ sx = {} }: DividerProps) {
  return <div style={{ height: 1, background: 'var(--b0)', flexShrink: 0, ...sx }} />
}
