import type { CSSProperties, ReactNode } from 'react'

interface LblProps {
  children?: ReactNode
  sx?: CSSProperties
}

export default function Lbl({ children, sx = {} }: LblProps) {
  return (
    <div
      style={{
        fontFamily: 'var(--fh)',
        fontWeight: 600,
        fontSize: 11,
        letterSpacing: '.1em',
        textTransform: 'uppercase',
        color: 'var(--t2)',
        ...sx
      }}
    >
      {children}
    </div>
  )
}
