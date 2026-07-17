interface TogProps {
  checked: boolean
  onChange: (val: boolean) => void
  disabled?: boolean
  label?: string
  'data-testid'?: string
  'aria-label'?: string
}

export default function Tog({ checked, onChange, disabled, label, ...rest }: TogProps) {
  return (
    <button
      onClick={() => !disabled && onChange(!checked)}
      disabled={disabled}
      aria-label={rest['aria-label'] ?? label ?? 'toggle'}
      aria-pressed={checked}
      data-testid={rest['data-testid']}
      style={{
        width: 42,
        height: 24,
        borderRadius: 12,
        border: 'none',
        cursor: disabled ? 'default' : 'pointer',
        flexShrink: 0,
        background: checked ? 'var(--teal)' : 'var(--b1)',
        position: 'relative',
        transition: 'background .18s',
        boxShadow: checked ? '0 0 8px rgba(0,196,154,.3)' : 'none'
      }}
    >
      <span
        style={{
          position: 'absolute',
          top: 3,
          left: checked ? 21 : 3,
          width: 18,
          height: 18,
          borderRadius: '50%',
          background: '#fff',
          transition: 'left .18s cubic-bezier(.34,1.56,.64,1)',
          boxShadow: '0 1px 3px rgba(0,0,0,.4)'
        }}
      />
    </button>
  )
}
