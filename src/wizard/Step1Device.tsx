import { useState } from 'react'
import { ArrowRight, Check, Copy, Info, X } from 'lucide-react'
import Btn from '../components/primitives/Btn'
import Lbl from '../components/primitives/Lbl'

interface Step1Props {
  onNext: (key: string, wallet: string) => void
  onBack: () => void
}

function normalizeFemKey(input: string): string | null {
  const trimmed = input.trim()
  if (!/^FEM-[A-Z0-9]{32}$/i.test(trimmed)) return null
  const body = trimmed.slice(4).toUpperCase()
  return `FEM-${body}`
}

export default function Step1Device({ onNext, onBack }: Step1Props) {
  const [key, setKey] = useState('')
  const [addr, setAddr] = useState('')
  const normalizedKey = normalizeFemKey(key)
  const keyValid = normalizedKey !== null
  const addrValid = addr.length === 58 && /^[A-Z2-7]+$/.test(addr)
  const canContinue = keyValid && addrValid

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
            marginBottom: 7
          }}
        >
          Device & Wallet
        </div>
        <div style={{ fontFamily: 'var(--fb)', fontSize: 13, color: 'var(--t1)' }}>
          Enter your FEM miner key and the Algorand address registered to this device.
        </div>
      </div>

      <div style={{ marginBottom: 16 }}>
        <Lbl sx={{ marginBottom: 7 }}>Miner Key</Lbl>
        <input
          className="inp"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          placeholder="FEM-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
          spellCheck={false}
          style={{
            width: '100%',
            padding: '10px 12px',
            background: 'var(--s2)',
            border: '1px solid var(--b1)',
            borderRadius: 'var(--radsm)',
            color: 'var(--txt)',
            fontFamily: 'var(--fm)',
            fontSize: 12,
            transition: 'border-color .15s'
          }}
        />
        {key.length > 4 && (
          <div
            style={{
              marginTop: 6,
              display: 'flex',
              alignItems: 'center',
              gap: 5,
              fontFamily: 'var(--fb)',
              fontSize: 12
            }}
          >
            {keyValid ? (
              <>
                <Check size={11} color="var(--teal)" strokeWidth={2.5} />
                <span style={{ color: 'var(--teal)' }}>Valid miner key</span>
              </>
            ) : (
              <>
                <X size={11} color="var(--red)" strokeWidth={2.5} />
                <span style={{ color: 'var(--red)' }}>Must be FEM- followed by 32 alphanumeric characters</span>
              </>
            )}
          </div>
        )}
      </div>

      <div style={{ marginBottom: 16 }}>
        <Lbl sx={{ marginBottom: 7 }}>Algorand Address</Lbl>
        <div style={{ position: 'relative' }}>
          <input
            className="inp"
            value={addr}
            onChange={(e) => setAddr(e.target.value.toUpperCase())}
            placeholder="Your 58-character reward wallet address…"
            spellCheck={false}
            style={{
              width: '100%',
              padding: '10px 38px 10px 12px',
              background: 'var(--s2)',
              border: '1px solid var(--b1)',
              borderRadius: 'var(--radsm)',
              color: 'var(--txt)',
              fontFamily: 'var(--fm)',
              fontSize: 12,
              transition: 'border-color .15s'
            }}
          />
          <Copy
            size={12}
            color="var(--t2)"
            style={{
              position: 'absolute',
              right: 10,
              top: '50%',
              transform: 'translateY(-50%)',
              pointerEvents: 'none'
            }}
          />
        </div>
        {addr.length > 2 && (
          <div
            style={{
              marginTop: 6,
              display: 'flex',
              alignItems: 'center',
              gap: 5,
              fontFamily: 'var(--fb)',
              fontSize: 12
            }}
          >
            {addrValid ? (
              <>
                <Check size={11} color="var(--teal)" strokeWidth={2.5} />
                <span style={{ color: 'var(--teal)' }}>Valid Algorand address</span>
              </>
            ) : (
              <>
                <X size={11} color="var(--red)" strokeWidth={2.5} />
                <span style={{ color: 'var(--red)' }}>Not valid — {addr.length}/58 chars</span>
              </>
            )}
          </div>
        )}
      </div>

      <div
        style={{
          background: 'var(--s2)',
          border: '1px solid var(--b0)',
          borderRadius: 'var(--rad)',
          overflow: 'hidden',
          marginBottom: 14
        }}
      >
        <div style={{ padding: '9px 13px', borderBottom: '1px solid var(--b0)' }}>
          <Lbl>Verification Stakes & Multipliers</Lbl>
        </div>
        {[
          ['6-month lock · 745 FRY 2.0', '3.0×', 'var(--teal)', 'var(--tealg)'],
          ['24-hour lock · 2,235 FRY 2.0', '1.5×', 'var(--amb)', 'rgba(240,165,0,.06)'],
          ['No verification stake', '1.0×', 'var(--t1)', 'transparent']
        ].map(([l, m, c, bg], i) => (
          <div
            key={i}
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              padding: '9px 13px',
              borderBottom: i < 2 ? '1px solid var(--b0)' : 'none',
              background: bg as string
            }}
          >
            <span style={{ fontFamily: 'var(--fb)', fontSize: 12, color: 'var(--t1)' }}>{l}</span>
            <span
              style={{
                fontFamily: 'var(--fm)',
                fontSize: 13,
                color: c as string,
                fontWeight: 500
              }}
            >
              {m}
            </span>
          </div>
        ))}
      </div>

      <div
        style={{
          background: 'var(--s2)',
          border: '1px solid var(--b1)',
          borderRadius: 'var(--rad)',
          padding: '9px 13px',
          marginBottom: 24,
          display: 'flex',
          gap: 8
        }}
      >
        <Info size={12} color="var(--t2)" style={{ marginTop: 1, flexShrink: 0 }} />
        <span style={{ fontFamily: 'var(--fb)', fontSize: 12, color: 'var(--t1)' }}>
          Verification stakes are managed on{' '}
          <span style={{ color: 'var(--teal)' }}>dashboard.frynetworks.com</span>. Your multiplier is applied automatically — no
          action needed here.
        </span>
      </div>

      <div style={{ marginTop: 'auto', display: 'flex', gap: 8 }}>
        <Btn v="g" onClick={onBack}>Back</Btn>
        <Btn v="p" onClick={() => { if (normalizedKey) onNext(normalizedKey, addr) }} disabled={!canContinue}>
          Continue <ArrowRight size={13} />
        </Btn>
      </div>
    </div>
  )
}
