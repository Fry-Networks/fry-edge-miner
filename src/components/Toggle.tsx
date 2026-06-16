interface ToggleProps {
  checked: boolean
  onChange: () => void
  disabled?: boolean
}

export function Toggle({ checked, onChange, disabled }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => { if (!disabled) onChange() }}
      className={`relative inline-flex h-6 w-11 shrink-0 rounded-full border-2 border-transparent transition-colors duration-150 focus-visible:outline focus-visible:outline-2 focus-visible:outline-fry-neon disabled:opacity-50 disabled:cursor-not-allowed ${
        checked ? 'bg-fry-neon' : 'bg-fry-border'
      } ${!disabled ? 'cursor-pointer' : ''}`}
      style={checked ? { boxShadow: '0 0 8px #00B69B' } : undefined}
    >
      <span
        className={`pointer-events-none inline-block h-5 w-5 rounded-full bg-white shadow-sm transform transition-transform duration-150 ${
          checked ? 'translate-x-5' : 'translate-x-0'
        }`}
      />
    </button>
  )
}
