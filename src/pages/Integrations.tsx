import IntCard from '../components/IntCard'
import Lbl from '../components/primitives/Lbl'
import type { MockIntegration } from '../lib/data'

interface IntegrationsProps {
  intgs: MockIntegration[]
  onToggle: (id: string) => void
}

export default function Integrations({ intgs, onToggle }: IntegrationsProps) {
  const active = intgs.filter((i) => i.enabled && i.healthy).length
  return (
    <div
      className="sc"
      style={{
        padding: '20px 24px',
        display: 'flex',
        flexDirection: 'column',
        gap: 12,
        overflowY: 'auto',
        height: '100%'
      }}
    >
      <div
        style={{
          background: 'var(--s2)',
          border: '1px solid var(--b0)',
          borderRadius: 'var(--rad)',
          padding: '11px 16px',
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center'
        }}
      >
        <span style={{ fontFamily: 'var(--fb)', fontSize: 13, color: 'var(--t1)' }}>
          Each <span style={{ color: 'var(--teal)', fontFamily: 'var(--fm)' }}>enabled</span> +{' '}
          <span style={{ color: 'var(--teal)', fontFamily: 'var(--fm)' }}>healthy</span> integration contributes 20% to your daily
          reward.
        </span>
        <div
          style={{
            fontFamily: 'var(--fm)',
            fontSize: 12,
            padding: '5px 11px',
            borderRadius: 'var(--radsm)',
            background: 'var(--tealg)',
            color: 'var(--teal)',
            flexShrink: 0,
            marginLeft: 12
          }}
        >
          {active}/5 · {active * 20}%
        </div>
      </div>
      <Lbl sx={{ marginBottom: 0 }}>Integrations</Lbl>
      {intgs.map((intg) => (
        <IntCard key={intg.id} intg={intg} onToggle={onToggle} />
      ))}
    </div>
  )
}
