import { Check } from 'lucide-react'
import NetSVG from '../components/NetSVG'
import { APP_VERSION } from '../lib/version'

const WLABELS = ['Welcome', 'Wallet', 'Integrations', 'Installing']

interface WizPanelProps {
  step: number
}

export default function WizPanel({ step }: WizPanelProps) {
  return (
    <div
      style={{
        width: 252,
        background: 'var(--s0)',
        borderRight: '1px solid var(--b0)',
        display: 'flex',
        flexDirection: 'column',
        padding: '36px 24px',
        position: 'relative',
        overflow: 'hidden',
        flexShrink: 0
      }}
    >
      <NetSVG />
      <div style={{ position: 'relative', zIndex: 1, marginBottom: 44 }}>
        <div
          style={{
            fontFamily: 'var(--fl)',
            fontWeight: 700,
            fontSize: 24,
            color: 'var(--txt)',
            letterSpacing: '.04em',
            lineHeight: 1
          }}
        >
          FRY
        </div>
        <div
          style={{
            fontFamily: 'var(--fh)',
            fontSize: 10,
            letterSpacing: '.28em',
            color: 'var(--t2)',
            marginTop: 3
          }}
        >
          EDGE MINER
        </div>
        <div
          style={{ fontFamily: 'var(--fm)', fontSize: 9, color: 'var(--t2)', marginTop: 5, opacity: 0.6 }}
        >
          v{APP_VERSION}
        </div>
      </div>
      <div style={{ position: 'relative', zIndex: 1, display: 'flex', flexDirection: 'column' }}>
        {WLABELS.map((lbl, i) => {
          const done = i < step
          const active = i === step
          return (
            <div key={i}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                <div
                  style={{
                    width: 25,
                    height: 25,
                    borderRadius: '50%',
                    flexShrink: 0,
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    background: done ? 'var(--teal)' : active ? 'var(--red)' : 'var(--s3)',
                    border: `2px solid ${done ? 'var(--teal)' : active ? 'var(--red)' : 'var(--b1)'}`,
                    transition: 'all .3s'
                  }}
                >
                  {done ? (
                    <Check size={11} color="#041f18" strokeWidth={3} />
                  ) : (
                    <span style={{ fontFamily: 'var(--fm)', fontSize: 9, color: active ? '#fff' : 'var(--t2)' }}>{i + 1}</span>
                  )}
                </div>
                <span
                  style={{
                    fontFamily: 'var(--fh)',
                    fontWeight: active ? 700 : 500,
                    fontSize: 13,
                    color: done ? 'var(--teal)' : active ? 'var(--txt)' : 'var(--t2)'
                  }}
                >
                  {lbl}
                </span>
              </div>
              {i < WLABELS.length - 1 && (
                <div
                  style={{
                    width: 2,
                    height: 16,
                    background: done ? 'var(--teald)' : 'var(--b0)',
                    marginLeft: 11.5,
                    transition: 'background .3s'
                  }}
                />
              )}
            </div>
          )
        })}
      </div>
      <div style={{ position: 'relative', zIndex: 1, marginTop: 'auto' }}>
        <div
          style={{
            fontFamily: 'var(--fh)',
            fontSize: 9,
            color: 'var(--t2)',
            letterSpacing: '.08em'
          }}
        >
          DECENTRALIZED INFRASTRUCTURE
        </div>
        <div
          style={{
            fontFamily: 'var(--fh)',
            fontSize: 9,
            color: 'var(--red)',
            letterSpacing: '.08em'
          }}
        >
          SIMPLIFIED.
        </div>
      </div>
    </div>
  )
}
