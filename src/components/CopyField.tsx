import { useState } from 'react'
import { Copy, Check } from 'lucide-react'

interface CopyFieldProps {
  value: string | null
  label?: string
  truncate?: number
}

export function CopyField({ value, label, truncate = 20 }: CopyFieldProps) {
  const [copied, setCopied] = useState(false)

  const handleCopy = () => {
    if (!value) return
    navigator.clipboard.writeText(value).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    })
  }

  const display = value ? (value.length > truncate ? value.slice(0, truncate) + '\u2026' : value) : '\u2014'

  return (
    <div className="space-y-1">
      {label && <p className="text-xs text-fry-text-muted">{label}</p>}
      <div className="flex items-center gap-2 bg-fry-surface-2 border border-fry-border rounded-lg px-3 py-2">
        <code className="font-mono text-xs text-fry-text-muted flex-1 truncate">{display}</code>
        <button
          type="button"
          onClick={handleCopy}
          disabled={!value}
          className={`shrink-0 transition-colors disabled:opacity-40 disabled:cursor-not-allowed ${
            copied ? 'text-fry-neon' : 'text-fry-text-muted hover:text-fry-text'
          }`}
          title={copied ? 'Copied!' : 'Copy to clipboard'}
        >
          {copied ? <Check className="w-3.5 h-3.5" /> : <Copy className="w-3.5 h-3.5" />}
        </button>
      </div>
    </div>
  )
}
