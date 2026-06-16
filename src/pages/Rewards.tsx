import { Activity, Percent, DollarSign } from 'lucide-react'
import { useRewards } from '../hooks/useRewards'
import { RewardBar } from '../components/RewardBar'
import { PoCSlotGrid } from '../components/PoCSlotGrid'
import { StatCard } from '../components/StatCard'
import { PageHeader } from '../components/PageHeader'

export default function Rewards() {
  const { rewards, loading } = useRewards()

  return (
    <div className="p-6 lg:p-8 space-y-6">
      <PageHeader
        title="Proportional Rewards"
        subtitle="Earnings distribution across active integrations"
      />

      {loading ? (
        <div className="space-y-4">
          {Array.from({ length: 3 }).map((_, idx) => (
            <div key={idx} className="bg-fry-surface border border-fry-border rounded-xl p-6 animate-pulse">
              <div className="h-5 bg-fry-border-subtle rounded" />
            </div>
          ))}
        </div>
      ) : rewards ? (
        <>
          {/* Stat cards */}
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <StatCard
              label="Active Rewards"
              value={rewards.summary.active_count}
              subtitle={`of ${rewards.summary.total_count} total`}
              icon={<Activity className="w-6 h-6" />}
              accent="#00B69B"
            />
            <StatCard
              label="Proportion"
              value={`${Math.round(rewards.summary.proportion * 100)}%`}
              icon={<Percent className="w-6 h-6" />}
              accent="#00B69B"
            />
            <StatCard
              label="Est. Daily"
              value={`$${rewards.summary.estimated_daily.toFixed(2)}`}
              icon={<DollarSign className="w-6 h-6" />}
              accent="#f97316"
            />
          </div>

          {/* Proportion bar */}
          <div className="bg-fry-surface border border-fry-border rounded-xl p-6">
            <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted mb-4">
              Reward Distribution
            </p>
            <RewardBar
              active={rewards.summary.active_count}
              total={rewards.summary.total_count}
              proportion={rewards.summary.proportion}
            />
          </div>

          {/* PoC Slot Grid */}
          <div className="bg-fry-surface border border-fry-border rounded-xl p-6">
            <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted mb-4">
              PoC Slot Status
            </p>
            <PoCSlotGrid slots={rewards.slots} />
          </div>

          {/* Reward history */}
          <div className="bg-fry-surface border border-fry-border rounded-xl p-6">
            <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted mb-4">
              Reward History
            </p>
            <table className="w-full text-xs tabular-nums">
              <thead>
                <tr className="border-b border-fry-border-subtle">
                  {['Epoch', 'Slots', 'Proportion', 'Est. Reward'].map((h) => (
                    <th key={h} className="text-left py-2 pr-4 text-fry-text-muted font-medium">{h}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                <tr>
                  <td colSpan={4} className="py-6 text-center text-fry-text-muted italic">
                    Historical reward data is not yet available.
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </>
      ) : (
        <div className="flex flex-col items-center justify-center py-24 text-center">
          <p className="text-fry-text-muted text-sm">Reward data unavailable</p>
          <p className="text-fry-text-muted/60 text-xs mt-1">Connect to the network to see rewards</p>
        </div>
      )}
    </div>
  )
}
