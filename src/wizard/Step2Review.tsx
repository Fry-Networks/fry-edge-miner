import { ArrowRight, Check } from 'lucide-react'
import Btn from '../components/primitives/Btn'
import Lbl from '../components/primitives/Lbl'
import { INTGS } from '../lib/data'

interface Step2Props {
  onNext: () => void
  onBack: () => void
}

export default function Step2Review({ onNext, onBack }: Step2Props) {
  const est = (59.52 * 3).toFixed(2)
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
      <div style={{ marginBottom: 20 }}>
        <div
          style={{
            fontFamily: 'var(--fh)',
            fontWeight: 700,
            fontSize: 26,
            color: 'var(--txt)',
            marginBottom: 7
          }}
        >
          Review Integrations
        </div>
        <div style={{ fontFamily: 'var(--fb)', fontSize: 13, color: 'var(--t1)' }}>
          All 5 integrations are installed with FEM. You can enable or disable them after setup — disabled
          integrations reduce your reward proportionally.
        </div>
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 7, marginBottom: 18 }}>
        {INTGS.map(({ id, name, tag, desc, Icon, col }) => (
          <div
            key={id}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 12,
              padding: '12px 13px',
              border: `1px solid ${col}35`,
              borderRadius: 'var(--rad)',
              background: `${col}08`
            }}
          >
            <div
              style={{
                width: 36,
                height: 36,
                borderRadius: 'var(--radsm)',
                background: `${col}18`,
                border: `1px solid ${col}30`,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                flexShrink: 0
              }}
            >
              <Icon size={16} color={col} />
            </div>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 7, marginBottom: 2 }}>
                <span style={{ fontFamily: 'var(--fh)', fontWeight: 700, fontSize: 13, color: 'var(--txt)' }}>{name}</span>
                <span
                  style={{
                    fontFamily: 'var(--fh)',
                    fontSize: 9,
                    fontWeight: 700,
                    letterSpacing: '.07em',
                    color: col,
                    opacity: 0.9
                  }}
                >
                  {tag}
                </span>
              </div>
              <div
                style={{
                  fontFamily: 'var(--fb)',
                  fontSize: 11,
                  color: 'var(--t2)',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap'
                }}
              >
                {desc}
              </div>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 7, flexShrink: 0 }}>
              <span style={{ fontFamily: 'var(--fm)', fontSize: 10, color: 'var(--teal)' }}>20%</span>
              <div
                style={{
                  width: 18,
                  height: 18,
                  borderRadius: '50%',
                  background: 'var(--teal)',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center'
                }}
              >
                <Check size={10} color="#041f18" strokeWidth={3} />
              </div>
            </div>
          </div>
        ))}
      </div>

      <div
        style={{
          background: 'var(--s2)',
          border: '1px solid var(--b1)',
          borderRadius: 'var(--rad)',
          padding: '12px 16px',
          marginBottom: 24,
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center'
        }}
      >
        <div>
          <Lbl sx={{ marginBottom: 3 }}>Est. daily reward at 5/5</Lbl>
          <span style={{ fontFamily: 'var(--fm)', fontSize: 20, color: 'var(--teal)' }}>~{est} fNODE</span>
        </div>
        <div style={{ textAlign: 'right' }}>
          <Lbl sx={{ marginBottom: 3 }}>Proportion</Lbl>
          <span style={{ fontFamily: 'var(--fm)', fontSize: 20, color: 'var(--teal)' }}>5/5 · 100%</span>
        </div>
      </div>

      <div style={{ marginTop: 'auto', display: 'flex', gap: 8 }}>
        <Btn v="g" onClick={onBack}>Back</Btn>
        <Btn v="p" onClick={onNext}>
          Begin Installation <ArrowRight size={13} />
        </Btn>
      </div>
    </div>
  )
}
