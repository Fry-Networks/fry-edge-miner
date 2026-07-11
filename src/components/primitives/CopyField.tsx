import { useState } from 'react'
import { Check, Copy } from 'lucide-react'

interface CopyFieldProps {
  val: string
}

export default function CopyField({ val }: CopyFieldProps) {
  const [copied, setCopied] = useState(false)
  const copy = () => {
    navigator.clipboard?.writeText?.(val)
    setCopied(true)
    setTimeout(() => setCopied(false), 1400)
  }
  return (
    <button
      onClick={copy}
      aria-label={copied ? 'Copied' : 'Copy to clipboard'}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 7,
        background: 'var(--s3)',
        border: '1px solid var(--b1)',
        borderRadius: 'var(--radsm)',
        padding: '6px 11px',
        cursor: 'pointer',
        color: 'var(--txt)',
        fontFamily: 'var(--fm)',
        fontSize: 11,
        maxWidth: 320,
        overflow: 'hidden',
        textOverflow: 'ellipsis',
        whiteSpace: 'nowrap'
      }}
    >
      <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{val}</span>
      {copied ? <Check size={11} color="var(--teal)" /> : <Copy size={11} color="var(--t2)" />}
    </button>
  )
}
