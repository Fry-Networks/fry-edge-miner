interface ToggleProps {
  checked: boolean
  onChange: () => void
  disabled?: boolean
}

export function Toggle({ checked, onChange, disabled }: ToggleProps) {
  const handleClick = () => {
    if (disabled) return
    onChange()
  }

  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={handleClick}
      className={`relative inline-flex h-5 w-9 shrink-0 rounded-full border-2 border-transparent transition-colors duration-150 focus-visible:outline focus-visible:outline-2 focus-visible:outline-fry-neon disabled:opacity-50 disabled:cursor-not-allowed ${
        checked ? 'bg-fry-neon' : 'bg-fry-border'
      } ${!disabled ? 'cursor-pointer' : ''}`}
    >
      <span
        className={`pointer-events-none inline-block h-4 w-4 rounded-full bg-white shadow-sm transform transition-transform duration-150 ${
          checked ? 'translate-x-4' : 'translate-x-0'
        }`}
      />
    </button>
  )
}
