interface DotProps {
  status?: 'run' | 'err' | 'stopped'
  size?: number
}

export default function Dot({ status, size = 7 }: DotProps) {
  const col = status === 'run' ? 'var(--teal)' : status === 'err' ? 'var(--red)' : 'var(--t2)'
  return (
    <span
      style={{
        width: size,
        height: size,
        borderRadius: '50%',
        background: col,
        display: 'inline-block',
        flexShrink: 0,
        animation: status === 'run' ? 'pr 2s ease infinite' : 'none'
      }}
    />
  )
}
