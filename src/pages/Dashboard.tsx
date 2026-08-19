import { Activity, Coins, MessageCircle, Puzzle, type LucideIcon } from 'lucide-react'
import Dot from '../components/primitives/Dot'
import EmptyState from '../components/primitives/EmptyState'
import Lbl from '../components/primitives/Lbl'
import PoCGrid from '../components/PoCGrid'
import StatCard from '../components/StatCard'
import TierBadge from '../components/primitives/TierBadge'
import Divider from '../components/primitives/Divider'
import { useRewards } from '../hooks/useRewards'
import { useReporting } from '../hooks/useReporting'
import { categoryCounts } from '../lib/availability'
import { activeFraction, proportionPct } from '../lib/integrationCount'
import type { IntegrationTier } from '../lib/integrationMeta'
import { SDK_REPORT_LINE } from '../lib/support'
import { officialCounts, sdkActiveLine, sdkCounts, splitByTier } from '../lib/tierSplit'

interface DashboardIntegration {
  id: string
  name: string
  Icon: LucideIcon
  col: string
  enabled: boolean
  healthy: boolean
  tier: IntegrationTier
  unavailable_reason?: string | null
}

interface DashboardProps {
  intgs: DashboardIntegration[]
}

/**
 * One integration tile. `compact` shrinks it for the community section: the
 * hierarchy between the two tiers is carried by density, not by a second
 * accent colour, so the official partners stay the thing you read first.
 */
function MiniCard({ intg, compact }: { intg: DashboardIntegration; compact: boolean }) {
  const { name, Icon, col, enabled, healthy } = intg
  const st = !enabled ? 'stopped' : healthy ? 'run' : 'err'
  const box = compact ? 24 : 30
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: compact ? 7 : 9,
        padding: compact ? '7px 10px' : '10px 12px',
        background: compact ? 'var(--s0)' : 'var(--s1)',
        border: '1px solid var(--b0)',
        borderRadius: 'var(--rad)',
        opacity: enabled ? 1 : 0.5
      }}
    >
      <div
        style={{
          width: box,
          height: box,
          borderRadius: 'var(--radsm)',
          background: `${col}12`,
          border: `1px solid ${col}22`,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          flexShrink: 0
        }}
      >
        <Icon size={compact ? 12 : 14} color={col} />
      </div>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          style={{
            fontFamily: 'var(--fh)',
            fontWeight: 600,
            fontSize: compact ? 11 : 12,
            color: 'var(--txt)',
            whiteSpace: 'nowrap',
            overflow: 'hidden',
            textOverflow: 'ellipsis'
          }}
        >
          {name}
        </div>
        <div style={{ fontFamily: 'var(--fb)', fontSize: compact ? 10 : 11, color: 'var(--t2)' }}>
          {!enabled ? 'Disabled' : healthy ? 'Running' : 'Unhealthy'}
        </div>
      </div>
      <Dot status={st} />
    </div>
  )
}

export default function Dashboard({ intgs }: DashboardProps) {
  const { rewards } = useRewards()
  const reporting = useReporting()
  // B2: truthful reporting banner — red when PoC/lease is persistently
  // failing, amber during transient retries.
  const notReporting =
    !!reporting?.registered &&
    (reporting.consecutive_poc_failures >= 3 || (!reporting.lease_active && !!reporting.lease_error))
  const reportingDegraded =
    !!reporting?.registered && !notReporting && reporting.consecutive_poc_failures > 0
  const summary = rewards.summary
  const active = intgs.filter((i) => i.enabled)
  // Count against what this machine can run, matching the PoC denominator.
  const available = categoryCounts(intgs).availableTotal
  const pct = String(proportionPct(active.length, available))
  // F2: presentation split only — `available`/`pct` above still feed the
  // reward breakdown from the full list, exactly as before.
  const { official: officialIntgs, sdk: sdkIntgs } = splitByTier(intgs)
  const official = officialCounts(intgs)
  const sdk = sdkCounts(intgs)
  const sdkLine = sdkActiveLine(sdk.activeCount)
  const slotHits = rewards.slots.filter((s) => s.done).length
  const estimated = summary ? summary.estimated_daily.toFixed(2) : '0.00'
  const rewardToken = summary ? summary.reward_token_name : '—'
  const rewardAsa = summary ? summary.reward_token_asa_id : '—'
  const baseReward = summary ? summary.base_reward.toFixed(2) : '0.00'
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
      {notReporting && (
        <div
          style={{
            padding: '10px 14px',
            background: 'var(--red)18',
            border: '1px solid var(--red)40',
            borderRadius: 'var(--rad)',
            fontFamily: 'var(--fb)',
            fontSize: 12,
            color: 'var(--red)'
          }}
        >
          <strong>Not reporting to Fry Networks.</strong>{' '}
          {reporting?.last_poc_error || reporting?.lease_error || 'PoC submissions are failing.'}{' '}
          Rewards pause while reporting is down — the status below may be stale.
        </div>
      )}
      {reportingDegraded && (
        <div
          style={{
            padding: '10px 14px',
            background: 'var(--amb)18',
            border: '1px solid var(--amb)40',
            borderRadius: 'var(--rad)',
            fontFamily: 'var(--fb)',
            fontSize: 12,
            color: 'var(--amb)'
          }}
        >
          Reporting hiccup — retrying automatically. Last error: {reporting?.last_poc_error}
        </div>
      )}
      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
        <StatCard
          Icon={Puzzle}
          label="Active Integrations"
          value={`${official.activeCount} / ${official.availableTotal}`}
          sub={`${pct}% reward proportion`}
          sub2={sdkLine ?? undefined}
          accent="var(--teal)"
        />
        <StatCard Icon={Coins} label="Daily Estimate" value={estimated} sub={`${rewardToken} (ASA ${rewardAsa})`} accent="var(--amb)" />
        <StatCard Icon={Activity} label="PoC Score" value={(slotHits / 144).toFixed(3)} sub={`${slotHits} / 144 slot hits today`} accent="var(--red)" />
      </div>

      {intgs.length === 0 ? (
        <div>
          <Lbl sx={{ marginBottom: 9 }}>Integration Status</Lbl>
          <EmptyState message="No integration data" />
        </div>
      ) : (
        <>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 9, marginBottom: 9 }}>
              <Lbl>Official Partners</Lbl>
              <TierBadge kind="official" />
              <span style={{ fontFamily: 'var(--fm)', fontSize: 11, color: 'var(--t2)' }}>
                {official.activeCount}/{official.availableTotal} active
              </span>
            </div>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill,minmax(185px,1fr))', gap: 8 }}>
              {officialIntgs.map((i) => (
                <MiniCard key={i.id} intg={i} compact={false} />
              ))}
            </div>
          </div>

          {sdkIntgs.length > 0 && (
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 9, marginBottom: 7, flexWrap: 'wrap' }}>
                <Lbl>Community / SDK</Lbl>
                <TierBadge kind="experimental" />
                <span style={{ fontFamily: 'var(--fm)', fontSize: 11, color: 'var(--t2)' }}>
                  {sdk.activeCount}/{sdk.availableTotal} active
                </span>
              </div>
              <div
                style={{
                  fontFamily: 'var(--fb)',
                  fontSize: 11,
                  color: 'var(--t1)',
                  display: 'flex',
                  alignItems: 'center',
                  gap: 4,
                  marginBottom: 8
                }}
              >
                <MessageCircle size={11} style={{ flexShrink: 0 }} /> {SDK_REPORT_LINE}
              </div>
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill,minmax(160px,1fr))', gap: 7 }}>
                {sdkIntgs.map((i) => (
                  <MiniCard key={i.id} intg={i} compact />
                ))}
              </div>
            </div>
          )}
        </>
      )}

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
            <span style={{ fontFamily: 'var(--fm)', fontSize: 11, color: 'var(--teal)' }}>{slotHits}/144</span>
          </div>
          <PoCGrid slots={rewards.slots} />
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
              <span style={{ fontFamily: 'var(--fb)', fontSize: 11, color: 'var(--t1)' }}>Active</span>
              <span style={{ fontFamily: 'var(--fm)', fontSize: 11, color: 'var(--teal)' }}>{activeFraction(active.length, available)}</span>
            </div>
            <div style={{ height: 4, background: 'var(--b1)', borderRadius: 2, overflow: 'hidden' }}>
              <div
                style={{
                  height: '100%',
                  width: `${proportionPct(active.length, available)}%`,
                  background: 'var(--teal)',
                  borderRadius: 2,
                  transition: 'width .5s ease'
                }}
              />
            </div>
          </div>
          <Divider sx={{ marginBottom: 10 }} />
          {[
            ['Base reward', summary ? `${baseReward} ${rewardToken}` : '—', 'var(--txt)'],
            ['Staking mult', summary ? `${summary.stake_multiplier.toFixed(1)}×` : '—', 'var(--teal)'],
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
            <span style={{ fontFamily: 'var(--fm)', fontSize: 14, color: 'var(--teal)' }}>—</span>
          </div>
        </div>
      </div>
    </div>
  )
}
