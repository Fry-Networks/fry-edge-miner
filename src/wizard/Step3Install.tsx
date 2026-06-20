import { useEffect, useRef, useState } from 'react'
import { ArrowRight, Check } from 'lucide-react'
import Btn from '../components/primitives/Btn'
import Divider from '../components/primitives/Divider'
import Lbl from '../components/primitives/Lbl'
interface InstallStage { label: string; ms: number }
const STAGES: InstallStage[] = [
  { label: 'Validating miner key', ms: 900 },
  { label: 'Registering with Fry Networks', ms: 1100 },
  { label: 'Downloading partner software', ms: 2000 },
  { label: 'Verifying SHA256 signatures', ms: 700 },
  { label: 'Starting integration services', ms: 900 },
]
import { makeName } from '../lib/names'

interface Step3Props {
  onDone: (name: string) => void
}

export default function Step3Install({ onDone }: Step3Props) {
  const [stage, setStage] = useState(-1)
  const [done, setDone] = useState(false)
  const deviceName = useRef(makeName()).current

  useEffect(() => {
    let idx = 0
    const run = () => {
      if (idx < STAGES.length) {
        setStage(idx)
        setTimeout(() => {
          idx++
          run()
        }, STAGES[idx]?.ms ?? 900)
      } else {
        setDone(true)
      }
    }
    const t = setTimeout(run, 400)
    return () => clearTimeout(t)
  }, [])

  if (done) {
    return (
      <div
        className="fu"
        style={{
          flex: 1,
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          padding: '36px 44px',
          textAlign: 'center'
        }}
      >
        <div
          style={{
            width: 68,
            height: 68,
            borderRadius: '50%',
            background: 'var(--tealg)',
            border: '2px solid var(--teal)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            marginBottom: 20,
            animation: 'pop .5s cubic-bezier(.34,1.56,.64,1) both'
          }}
        >
          <Check size={30} color="var(--teal)" strokeWidth={2} />
        </div>
        <div
          style={{
            fontFamily: 'var(--fh)',
            fontWeight: 700,
            fontSize: 26,
            color: 'var(--txt)',
            marginBottom: 6
          }}
        >
          Installation Complete
        </div>
        <div
          style={{
            fontFamily: 'var(--fb)',
            fontSize: 13,
            color: 'var(--t1)',
            marginBottom: 24,
            maxWidth: 320
          }}
        >
          Your device is registered. Rewards begin accruing in the next PoC cycle.
        </div>
        <div
          style={{
            background: 'var(--s2)',
            border: '1px solid var(--b1)',
            borderRadius: 'var(--rad)',
            padding: '16px 20px',
            marginBottom: 24,
            width: '100%',
            maxWidth: 400
          }}
        >
          <div
            style={{
              fontFamily: 'var(--fl)',
              fontWeight: 700,
              fontSize: 20,
              color: 'var(--teal)',
              letterSpacing: '.04em',
              marginBottom: 4
            }}
          >
            {deviceName}
          </div>
          <div style={{ fontFamily: 'var(--fb)', fontSize: 11, color: 'var(--t2)', marginBottom: 12 }}>Your device&apos;s display name</div>
          <Divider sx={{ marginBottom: 10 }} />
          <Lbl sx={{ marginBottom: 5 }}>Miner Key</Lbl>
          <code
            style={{
              fontFamily: 'var(--fm)',
              fontSize: 11,
              color: 'var(--txt)',
              lineHeight: 1.5,
              display: 'block',
              wordBreak: 'break-all'
            }}
          >
            FEM-b9e489c8a32d5547bbb7c363baaf733e
          </code>
        </div>
        <Btn
          v="t"
          onClick={() => onDone(deviceName)}
          sx={{ fontSize: 14, padding: '10px 24px' }}
        >
          Open Dashboard <ArrowRight size={14} />
        </Btn>
      </div>
    )
  }

  const pct = stage < 0 ? 0 : ((stage + 1) / STAGES.length) * 100
  return (
    <div
      className="fu"
      style={{
        flex: 1,
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'center',
        padding: '36px 44px'
      }}
    >
      <div
        style={{
          fontFamily: 'var(--fh)',
          fontWeight: 700,
          fontSize: 26,
          color: 'var(--txt)',
          marginBottom: 7
        }}
      >
        Installing
      </div>
      <div style={{ fontFamily: 'var(--fb)', fontSize: 13, color: 'var(--t1)', marginBottom: 32 }}>
        Setting up your FEM device and partner integrations…
      </div>
      <div
        style={{
          height: 3,
          background: 'var(--b1)',
          borderRadius: 2,
          marginBottom: 28,
          overflow: 'hidden'
        }}
      >
        <div
          style={{
            height: '100%',
            width: `${pct}%`,
            background: 'linear-gradient(90deg,var(--red),var(--teal))',
            borderRadius: 2,
            transition: 'width .8s cubic-bezier(.65,0,.35,1)',
            boxShadow: '0 0 8px rgba(0,196,154,.4)'
          }}
        />
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        {STAGES.map(({ label }, i) => {
          const d = i < stage
          const a = i === stage
          return (
            <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <div
                style={{
                  width: 19,
                  height: 19,
                  borderRadius: '50%',
                  flexShrink: 0,
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  background: d ? 'var(--teal)' : 'var(--s3)',
                  border: `2px solid ${d ? 'var(--teal)' : a ? 'var(--red)' : 'var(--b1)'}`
                }}
              >
                {d ? (
                  <Check size={9} color="#041f18" strokeWidth={3} />
                ) : a ? (
                  <div
                    style={{
                      width: 5,
                      height: 5,
                      borderRadius: '50%',
                      background: 'var(--red)',
                      animation: 'breathe .8s ease-in-out infinite'
                    }}
                  />
                ) : null}
              </div>
              <span
                style={{
                  fontFamily: 'var(--fb)',
                  fontSize: 12,
                  color: d ? 'var(--teal)' : a ? 'var(--txt)' : 'var(--t2)',
                  transition: 'color .3s'
                }}
              >
                {label}
              </span>
            </div>
          )
        })}
      </div>
    </div>
  )
}
