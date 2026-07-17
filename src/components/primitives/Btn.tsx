import type { CSSProperties, ReactNode } from 'react'

interface BtnProps {
  v?: 'p' | 't' | 'g'
  onClick?: () => void
  children?: ReactNode
  disabled?: boolean
  sx?: CSSProperties
  full?: boolean
  'data-testid'?: string
}

export default function Btn({ v = 'p', onClick, children, disabled, sx = {}, full, ...rest }: BtnProps) {
  const base: CSSProperties = {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 6,
    border: 'none',
    cursor: disabled ? 'not-allowed' : 'pointer',
    fontFamily: 'var(--fh)',
    fontWeight: 700,
    fontSize: 13,
    letterSpacing: '.05em',
    borderRadius: 'var(--radsm)',
    padding: '8px 18px',
    transition: 'all .15s',
    opacity: disabled ? 0.45 : 1,
    width: full ? '100%' : undefined,
    ...sx
  }

  if (v === 'p') {
    return (
      <button className="bp" onClick={onClick} disabled={disabled} style={{ ...base, background: 'var(--red)', color: '#fff' }} data-testid={rest['data-testid']}>
        {children}
      </button>
    )
  }
  if (v === 't') {
    return (
      <button className="bt" onClick={onClick} disabled={disabled} style={{ ...base, background: 'var(--teal)', color: '#041f18' }} data-testid={rest['data-testid']}>
        {children}
      </button>
    )
  }
  return (
    <button className="bg" onClick={onClick} disabled={disabled} style={{ ...base, background: 'var(--s2)', color: 'var(--t1)', border: '1px solid var(--b1)' }} data-testid={rest['data-testid']}>
      {children}
    </button>
  )
}
