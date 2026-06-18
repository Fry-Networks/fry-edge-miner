import type { ReactNode } from 'react'

type TagVariant = 'def' | 'run' | 'warn' | 'err' | 'paid' | 'info'

interface TagProps {
  v?: TagVariant
  children?: ReactNode
}

export default function Tag({ v = 'def', children }: TagProps) {
  const C: Record<TagVariant, { bg: string; co: string }> = {
    def: { bg: 'var(--s3)', co: 'var(--t2)' },
    run: { bg: 'var(--tealg)', co: 'var(--teal)' },
    warn: { bg: 'rgba(240,165,0,.12)', co: 'var(--amb)' },
    err: { bg: 'var(--redg)', co: 'var(--red)' },
    paid: { bg: 'rgba(34,197,94,.12)', co: '#22c55e' },
    info: { bg: 'rgba(74,158,255,.12)', co: 'var(--blu)' }
  }
  const c = C[v] ?? C.def
  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        fontFamily: 'var(--fh)',
        fontWeight: 700,
        fontSize: 10,
        letterSpacing: '.08em',
        textTransform: 'uppercase',
        padding: '2px 7px',
        borderRadius: 'var(--radsm)',
        background: c.bg,
        color: c.co
      }}
    >
      {children}
    </span>
  )
}
