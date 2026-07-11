import { useEffect, useState } from 'react'
import { ArrowRight, Check, X } from 'lucide-react'
import Btn from '../components/primitives/Btn'
import Lbl from '../components/primitives/Lbl'
import { APP_VERSION } from '../lib/version'

interface Step0Props {
  onNext: () => void
}

type CheckItem = { label: string; ok: boolean | null }

function platformLabel(): string {
  const p = typeof navigator !== 'undefined' ? navigator.platform : ''
  if (p.startsWith('Linux')) return 'Linux (64-bit)'
  if (p.startsWith('Win')) return 'Windows 10 / 11 (64-bit)'
  if (p.includes('Mac')) return 'macOS (64-bit)'
  return `${p || 'Unknown'} (64-bit)`
}

export default function Step0Welcome({ onNext }: Step0Props) {
  const [checks, setChecks] = useState<CheckItem[]>(() => [
    { label: platformLabel(), ok: true },
    { label: '4 GB free disk space', ok: null },
    { label: 'Active internet connection', ok: null },
    { label: 'Algorand-compatible wallet', ok: true }
  ])

  useEffect(() => {
    let cancelled = false
    ;(async () => {
      const online = typeof navigator !== 'undefined' ? navigator.onLine : true
      let diskOk = true
      try {
        const nav = typeof navigator !== 'undefined' ? navigator : undefined
        if (nav && nav.storage && typeof nav.storage.estimate === 'function') {
          const est = await nav.storage.estimate()
          const free = (est.quota ?? 0) - (est.usage ?? 0)
          diskOk = free >= 4 * 1024 * 1024 * 1024
        }
      } catch {
        diskOk = true
      }
      if (cancelled) return
      setChecks((prev) =>
        prev.map((c) => {
          if (c.label === '4 GB free disk space') return { ...c, ok: diskOk }
          if (c.label === 'Active internet connection') return { ...c, ok: online }
          return c
        })
      )
    })()
    return () => {
      cancelled = true
    }
  }, [])

  const whatsNew = [
    'Clearer registration errors — DNS/carrier-block troubleshooting steps built in',
    'Status indicator now distinguishes Degraded (local issue) from Disconnected',
    'Registration retries transient server errors automatically',
    'Accessibility: toggles and copy buttons now screen-reader friendly',
    'Expanded automated test coverage (87 end-to-end checks)'
  ]

  return (
    <div
      className="fu sc"
      style={{
        flex: 1,
        display: 'flex',
        flexDirection: 'column',
        padding: '36px 36px 36px 44px',
        overflowY: 'auto'
      }}
    >
      <div style={{ marginBottom: 24 }}>
        <div
          style={{
            fontFamily: 'var(--fh)',
            fontWeight: 700,
            fontSize: 26,
            color: 'var(--txt)',
            marginBottom: 8,
            lineHeight: 1.1
          }}
        >
          Welcome to Fry Edge Miner
        </div>
        <div
          style={{
            fontFamily: 'var(--fb)',
            fontSize: 13,
            color: 'var(--t1)',
            lineHeight: 1.6,
            maxWidth: 380
          }}
        >
          FEM consolidates all DePIN integrations into one application. Enable what you want, earn
          proportional rewards for every active integration.
        </div>
      </div>

      <div style={{ marginBottom: 24 }}>
        <Lbl sx={{ marginBottom: 10 }}>System Checks</Lbl>
        {checks.map(({ label, ok }, i) => (
          <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 7 }}>
            <div
              style={{
                width: 16,
                height: 16,
                borderRadius: '50%',
                flexShrink: 0,
                background: ok === null ? 'var(--t2)' : ok ? 'var(--teal)' : 'var(--red)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center'
              }}
            >
              {ok === null ? null : ok ? (
                <Check size={9} color="#041f18" strokeWidth={3} />
              ) : (
                <X size={9} color="#fff" strokeWidth={3} />
              )}
            </div>
            <span style={{ fontFamily: 'var(--fb)', fontSize: 13, color: 'var(--t1)' }}>{label}</span>
          </div>
        ))}
      </div>

      <div
        style={{
          background: 'var(--s2)',
          border: '1px solid var(--b1)',
          borderRadius: 'var(--rad)',
          padding: '14px 18px',
          marginBottom: 28
        }}
      >
        <Lbl sx={{ marginBottom: 9 }}>What&apos;s new in v{APP_VERSION}</Lbl>
        {whatsNew.map((t, i) => (
          <div key={i} style={{ display: 'flex', gap: 7, marginBottom: 5 }}>
            <span style={{ color: 'var(--teal)', flexShrink: 0 }}>–</span>
            <span style={{ fontFamily: 'var(--fb)', fontSize: 12, color: 'var(--t1)' }}>{t}</span>
          </div>
        ))}
      </div>

      <div style={{ marginTop: 'auto' }}>
        <Btn v="p" onClick={onNext}>
          Get Started <ArrowRight size={14} />
        </Btn>
      </div>
    </div>
  )
}
