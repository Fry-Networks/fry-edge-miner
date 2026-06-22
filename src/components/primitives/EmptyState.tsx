interface EmptyStateProps {
  message: string
  sub?: string
}

export default function EmptyState({ message, sub }: EmptyStateProps) {
  return (
    <div
      style={{
        padding: '18px 16px',
        background: 'var(--s1)',
        border: '1px solid var(--b0)',
        borderRadius: 'var(--rad)',
        textAlign: 'center'
      }}
    >
      <div style={{ fontFamily: 'var(--fb)', fontSize: 13, color: 'var(--t2)' }}>{message}</div>
      {sub && (
        <div style={{ fontFamily: 'var(--fb)', fontSize: 11, color: 'var(--t2)', opacity: 0.7, marginTop: 4 }}>
          {sub}
        </div>
      )}
    </div>
  )
}
