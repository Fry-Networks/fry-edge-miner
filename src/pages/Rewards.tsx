import { Coins, Shield, TrendingUp } from 'lucide-react'
import Lbl from '../components/primitives/Lbl'
import StatCard from '../components/StatCard'
import Tag from '../components/primitives/Tag'
import { GATES } from '../lib/integrationMeta'
import { useRewards, type HourlyGates } from '../hooks/useRewards'

export default function Rewards() {
  const { rewards } = useRewards()
  const { rows, hourlyGates, summary } = rewards
  const rewardToken = summary ? summary.reward_token_name : '—'
  const fullDayEst = summary ? summary.base_reward.toFixed(2) : '—'
  const totalEarned = rows.reduce((sum, r) => sum + r.reward, 0).toFixed(2)

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
        <StatCard Icon={Coins} label="Total Earned" value={totalEarned} sub={`${rewardToken} lifetime`} accent="var(--teal)" />
        <StatCard
          Icon={TrendingUp}
          label="Full Day Est."
          value={fullDayEst}
          sub={`${rewardToken} at full proportion`}
          accent="var(--amb)"
        />
        <StatCard Icon={Shield} label="Staking Tier" value={summary ? `${summary.stake_multiplier.toFixed(1)}×` : '—'} sub={summary ? `${summary.stake_label} stake active` : '—'} accent="var(--red)" />
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
            {rows.map((r, i) => (
              <tr
                key={r.date}
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
                  {r.reward > 0 ? `${r.reward} ${rewardToken}` : '—'}
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
              ['pass', 'var(--teal)', 1],
              ['pending', 'var(--b0)', 0.25]
            ].map(([l, c, o]) => (
              <div key={l as string} style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                <div style={{ width: 7, height: 7, background: c as string, borderRadius: 1, opacity: o as number }} />
                <span style={{ fontFamily: 'var(--fb)', fontSize: 10, color: 'var(--t2)' }}>{l}</span>
              </div>
            ))}
          </div>
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          {GATES.map((g) => {
            const gateKey = (g === 'mac' ? 'mac_match' : g) as keyof HourlyGates
            return (
              <div key={g} style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                <div style={{
                  width: 32,
                  fontFamily: 'var(--fm)',
                  fontSize: 8,
                  color: 'var(--t2)',
                  textAlign: 'right',
                  flexShrink: 0
                }}>
                  {g}
                </div>
                <div style={{ display: 'grid', gridTemplateColumns: 'repeat(24,1fr)', gap: 2, flex: 1 }}>
                  {Array.from({ length: 24 }, (_, h) => {
                    const hour = hourlyGates[h]
                    const pass = hour ? hour[gateKey] : false
                    return (
                      <div
                        key={`${g}-${h}`}
                        style={{
                          height: 10,
                          borderRadius: 1,
                          background: !pass ? 'var(--b0)' : 'var(--teal)',
                          opacity: !pass ? 0.25 : 1
                        }}
                      />
                    )
                  })}
                </div>
              </div>
            )
          })}
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 4, marginTop: 4 }}>
          <div style={{ width: 32, flexShrink: 0 }} />
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(24,1fr)', gap: 2, flex: 1 }}>
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
    </div>
  )
}
