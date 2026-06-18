import type { PocSlot } from '../lib/data'

interface PoCGridProps {
  slots: PocSlot[]
}

export default function PoCGrid({ slots }: PoCGridProps) {
  return (
    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(24,1fr)', gap: 2, marginBottom: 5 }}>
      {slots.map(({ done, pass }, i) => (
        <div
          key={i}
          style={{
            height: 8,
            borderRadius: 1,
            background: !done ? 'var(--b0)' : pass ? 'var(--teal)' : 'var(--red)',
            opacity: !done ? 0.25 : 1
          }}
        />
      ))}
    </div>
  )
}
