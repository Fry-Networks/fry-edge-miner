import { ArrowRight, Check, X } from 'lucide-react'
import Btn from '../components/primitives/Btn'
import Lbl from '../components/primitives/Lbl'

interface Step0Props {
  onNext: () => void
}

export default function Step0Welcome({ onNext }: Step0Props) {
  const checks: [string, boolean][] = [
    ['Windows 10 / 11 (64-bit)', true],
    ['4 GB free disk space', true],
    ['Active internet connection', true],
    ['Algorand-compatible wallet', true]
  ]

  const whatsNew = [
    'Single binary replaces FryHub + 5 installer types',
    'Toggleable integrations with 20% proportional rewards each',
    'Tauri v2 Rust backend — 5× lighter than Python',
    'Ed25519 auto-updates via hardwareapi version checks',
    "Tauri Secure Storage replaces FryHub's PBKDF2+Fernet"
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
        {checks.map(([l, ok], i) => (
          <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 7 }}>
            <div
              style={{
                width: 16,
                height: 16,
                borderRadius: '50%',
                flexShrink: 0,
                background: ok ? 'var(--teal)' : 'var(--red)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center'
              }}
            >
              {ok ? <Check size={9} color="#041f18" strokeWidth={3} /> : <X size={9} color="#fff" strokeWidth={3} />}
            </div>
            <span style={{ fontFamily: 'var(--fb)', fontSize: 13, color: 'var(--t1)' }}>{l}</span>
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
        <Lbl sx={{ marginBottom: 9 }}>What&apos;s new in v0.2.3</Lbl>
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
