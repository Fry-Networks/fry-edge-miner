import { Coins, Shield, TrendingUp } from 'lucide-react'
import Lbl from '../components/primitives/Lbl'
import StatCard from '../components/StatCard'
import Tag from '../components/primitives/Tag'
import type { PocSlot, RewardRow } from '../lib/data'
import { GATES } from '../lib/data'

export default function Rewards() {
  const RWDS: RewardRow[] = [
    { date: 'Jun 13', reward: 6.41, slots: 62, factor: 0.108, status: 'paid' },
    { date: 'Jun 12', reward: 0, slots: 0, factor: 0, status: 'none' },
    { date: 'Jun 11', reward: 0, slots: 0, factor: 0, status: 'none' },
    { date: 'Jun 10', reward: 5.88, slots: 58, factor: 0.099, status: 'paid' },
    { date: 'Jun 9', reward: 6.12, slots: 61, factor: 0.104, status: 'paid' }
  ]

  const POC: PocSlot[] = Array.from({ length: 144 }, (_, i) => ({
    done: i < 62,
    pass: i < 62 ? Math.random() > 0.1 : null
  }))

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
        <StatCard Icon={Coins} label="Total Earned" value="24.41" sub="fNODE lifetime" accent="var(--teal)" />
        <StatCard
          Icon={TrendingUp}
          label="Full Day Est."
          value="14.88"
          sub="fNODE at 4/5 integrations"
          accent="var(--amb)"
        />
        <StatCard Icon={Shield} label="Staking Tier" value="3.0×" sub="FRY 2.0 stake active" accent="var(--red)" />
      </div>

      <div
        style={{
          background: 'var(--s1)',
          border: '1px solid var(--b0)',
          borderRadius: 'var(--rad)',
          overflow: 'hidden'
        }}
      >
        <div
          style={{
            padding: '11px 16px',
            borderBottom: '1px solid var(--b0)',
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center'
          }}
        >
          <div style={{ fontFamily: 'var(--fh)', fontWeight: 700, fontSize: 13, color: 'var(--txt)' }}>Reward History</div>
          <Tag v="info">Epoch 1 live</Tag>
        </div>
        <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 13 }}>
          <thead>
            <tr style={{ background: 'var(--s0)' }}>
              {['Date', 'Reward', 'Slots', 'Factor', 'Status'].map((h) => (
                <th key={h} style={{ padding: '7px 14px', textAlign: 'left', borderBottom: '1px solid var(--b0)' }}>
                  <Lbl>{h}</Lbl>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {RWDS.map((r, i) => (
              <tr
                key={i}
                className="rr"
                style={{
                  borderBottom: '1px solid var(--b0)',
                  background: i % 2 ? 'var(--s0)' : 'transparent',
                  transition: 'background .12s'
                }}
              >
                <td style={{ padding: '10px 14px', fontFamily: 'var(--fb)', color: 'var(--txt)' }}>{r.date}</td>
                <td
                  style={{
                    padding: '10px 14px',
                    fontFamily: 'var(--fm)',
                    fontSize: 13,
                    color: r.reward > 0 ? 'var(--teal)' : 'var(--t2)',
                    fontWeight: r.reward > 0 ? 500 : 400
                  }}
                >
                  {r.reward > 0 ? `${r.reward} fNODE` : '—'}
                </td>
                <td
                  style={{
                    padding: '10px 14px',
                    fontFamily: 'var(--fm)',
                    fontSize: 13,
                    color: 'var(--t1)'
                  }}
                >
                  {r.slots}/144
                </td>
                <td
                  style={{
                    padding: '10px 14px',
                    fontFamily: 'var(--fm)',
                    fontSize: 13,
                    color: 'var(--t2)'
                  }}
                >
                  {r.factor.toFixed(3)}
                </td>
                <td style={{ padding: '10px 14px' }}>
                  {r.status === 'paid' ? <Tag v="paid">Paid</Tag> : <Tag v="def">No data</Tag>}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
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
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            marginBottom: 12
          }}
        >
          <div>
            <div style={{ fontFamily: 'var(--fh)', fontWeight: 600, fontSize: 12, color: 'var(--txt)' }}>PoC Heatmap — 6 gates × 24 hours</div>
            <div style={{ fontFamily: 'var(--fb)', fontSize: 11, color: 'var(--t2)' }}>Today · {GATES.join(' · ')}</div>
          </div>
          <div style={{ display: 'flex', gap: 10 }}>
            {[
              ['pass', 'var(--teal)'],
              ['fail', 'var(--red)'],
              ['pending', 'var(--b1)']
            ].map(([l, c]) => (
              <div key={l} style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                <div style={{ width: 7, height: 7, background: c, borderRadius: 1 }} />
                <span style={{ fontFamily: 'var(--fb)', fontSize: 10, color: 'var(--t2)' }}>{l}</span>
              </div>
            ))}
          </div>
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(24,1fr)', gap: 2 }}>
          {Array.from({ length: 24 }, (_, h) =>
            GATES.map((g, gi) => {
              const s = POC[h * 6 + gi] || { done: false, pass: null }
              return (
                <div
                  key={`${h}-${g}`}
                  style={{
                    height: 10,
                    borderRadius: 1,
                    background: !s.done ? 'var(--b0)' : s.pass ? 'var(--teal)' : 'var(--red)',
                    opacity: !s.done ? 0.25 : 1
                  }}
                />
              )
            })
          )}
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(24,1fr)', gap: 2, marginTop: 4 }}>
          {Array.from({ length: 24 }, (_, h) => (
            <div
              key={h}
              style={{
                fontFamily: 'var(--fm)',
                fontSize: 7,
                color: 'var(--t2)',
                textAlign: 'center'
              }}
            >
              {h % 6 === 0 ? `${String(h).padStart(2, '0')}h` : ''}
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}
