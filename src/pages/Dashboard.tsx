import { Activity, Coins, Puzzle } from 'lucide-react'
import Dot from '../components/primitives/Dot'
import Lbl from '../components/primitives/Lbl'
import PoCGrid from '../components/PoCGrid'
import StatCard from '../components/StatCard'
import Divider from '../components/primitives/Divider'
import type { MockIntegration } from '../lib/data'
import { POC } from '../lib/data'

interface DashboardProps {
  intgs: MockIntegration[]
}

export default function Dashboard({ intgs }: DashboardProps) {
  const active = intgs.filter((i) => i.enabled && i.healthy)
  const pct = ((active.length / intgs.length) * 100).toFixed(0)
  return (
    <div
      className="sc"
      style={{
        padding: '20px 24px',
        display: 'flex',
        flexDirection: 'column',
        gap: 16,
        overflowY: 'auto',
        height: '100%'
      }}
    >
      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
        <StatCard
          Icon={Puzzle}
          label="Active Integrations"
          value={`${active.length} / 5`}
          sub={`${pct}% reward proportion`}
          accent="var(--teal)"
        />
        <StatCard Icon={Coins} label="Daily Estimate" value="14.88" sub="fNODE (ASA 2485202024)" accent="var(--amb)" />
        <StatCard Icon={Activity} label="PoC Score" value="0.108" sub="62 / 144 slot hits today" accent="var(--red)" />
      </div>

      <div>
        <Lbl sx={{ marginBottom: 9 }}>Integration Status</Lbl>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill,minmax(185px,1fr))', gap: 8 }}>
          {intgs.map(({ id, name, Icon, col, enabled, healthy }) => {
            const st = !enabled ? 'stopped' : healthy ? 'run' : 'err'
            return (
              <div
                key={id}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 9,
                  padding: '10px 12px',
                  background: 'var(--s1)',
                  border: '1px solid var(--b0)',
                  borderRadius: 'var(--rad)',
                  opacity: enabled ? 1 : 0.5
                }}
              >
                <div
                  style={{
                    width: 30,
                    height: 30,
                    borderRadius: 'var(--radsm)',
                    background: `${col}12`,
                    border: `1px solid ${col}22`,
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    flexShrink: 0
                  }}
                >
                  <Icon size={14} color={col} />
                </div>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontFamily: 'var(--fh)', fontWeight: 600, fontSize: 12, color: 'var(--txt)' }}>{name}</div>
                  <div style={{ fontFamily: 'var(--fb)', fontSize: 11, color: 'var(--t2)' }}>{!enabled ? 'Disabled' : healthy ? 'Running' : 'Unhealthy'}</div>
                </div>
                <Dot status={st} />
              </div>
            )
          })}
        </div>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
        <div
          style={{
            background: 'var(--s1)',
            border: '1px solid var(--b0)',
            borderRadius: 'var(--rad)',
            padding: '14px 16px'
          }}
        >
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              marginBottom: 10
            }}
          >
            <div>
              <div style={{ fontFamily: 'var(--fh)', fontWeight: 600, fontSize: 12, color: 'var(--txt)' }}>PoC Slots</div>
              <div style={{ fontFamily: 'var(--fb)', fontSize: 11, color: 'var(--t2)' }}>Today · 144 slots</div>
            </div>
            <span style={{ fontFamily: 'var(--fm)', fontSize: 11, color: 'var(--teal)' }}>62/144</span>
          </div>
          <PoCGrid slots={POC} />
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            {['00:00', '06:00', '12:00', '18:00', '23:50'].map((t) => (
              <span key={t} style={{ fontFamily: 'var(--fm)', fontSize: 8, color: 'var(--t2)' }}>{t}</span>
            ))}
          </div>
        </div>

        <div
          style={{
            background: 'var(--s1)',
            border: '1px solid var(--b0)',
            borderRadius: 'var(--rad)',
            padding: '14px 16px'
          }}
        >
          <div
            style={{
              fontFamily: 'var(--fh)',
              fontWeight: 600,
              fontSize: 12,
              color: 'var(--txt)',
              marginBottom: 12
            }}
          >
            Reward Breakdown
          </div>
          <div style={{ marginBottom: 12 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4 }}>
              <span style={{ fontFamily: 'var(--fb)', fontSize: 11, color: 'var(--t1)' }}>Proportion</span>
              <span style={{ fontFamily: 'var(--fm)', fontSize: 11, color: 'var(--teal)' }}>{active.length}/5</span>
            </div>
            <div style={{ height: 4, background: 'var(--b1)', borderRadius: 2, overflow: 'hidden' }}>
              <div
                style={{
                  height: '100%',
                  width: `${(active.length / 5) * 100}%`,
                  background: 'var(--teal)',
                  borderRadius: 2,
                  transition: 'width .5s ease'
                }}
              />
            </div>
          </div>
          <Divider sx={{ marginBottom: 10 }} />
          {[
            ['Base reward', '59.52 fNODE', 'var(--txt)'],
            ['Staking mult', '3.0×', 'var(--teal)'],
            ['Proportion', `${pct}%`, 'var(--txt)'],
            ['BYOD factor', '1.0×', 'var(--t1)']
          ].map(([l, v, c]) => (
            <div key={l as string} style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 6 }}>
              <span style={{ fontFamily: 'var(--fb)', fontSize: 11, color: 'var(--t2)' }}>{l as string}</span>
              <span style={{ fontFamily: 'var(--fm)', fontSize: 11, color: c as string }}>{v as string}</span>
            </div>
          ))}
          <Divider sx={{ margin: '8px 0' }} />
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <span style={{ fontFamily: 'var(--fh)', fontWeight: 700, fontSize: 12, color: 'var(--txt)' }}>Yesterday</span>
            <span style={{ fontFamily: 'var(--fm)', fontSize: 14, color: 'var(--teal)' }}>6.41 fNODE</span>
          </div>
        </div>
      </div>
    </div>
  )
}
