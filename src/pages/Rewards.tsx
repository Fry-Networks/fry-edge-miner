import { useRewards } from '../hooks/useRewards'
import { RewardBar } from '../components/RewardBar'
import { PoCSlotGrid } from '../components/PoCSlotGrid'
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
            <div
              key={idx}
              className="bg-fry-surface border border-fry-border rounded-xl p-6 animate-pulse"
            >
              <div className="h-5 bg-fry-border-subtle rounded" />
            </div>
          ))}
        </div>
      ) : rewards ? (
        <>
          {/* Summary Cards */}
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <div className="bg-fry-surface border border-fry-border rounded-xl p-6">
              <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted mb-2">Active Rewards</p>
              <p className="text-3xl font-bold tabular-nums text-fry-neon">
                {rewards.summary.active_count}
              </p>
            </div>
            <div className="bg-fry-surface border border-fry-border rounded-xl p-6">
              <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted mb-2">Proportion</p>
              <p className="text-3xl font-bold tabular-nums text-fry-neon">
                {Math.round(rewards.summary.proportion * 100)}%
              </p>
            </div>
            <div className="bg-fry-surface border border-fry-border rounded-xl p-6">
              <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted mb-2">Est. Daily</p>
              <p className="text-3xl font-bold tabular-nums text-fry-neon">
                ${rewards.summary.estimated_daily.toFixed(2)}
              </p>
            </div>
          </div>

          {/* Reward Bar */}
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

          {/* Reward History Placeholder */}
          <div className="bg-fry-surface border border-fry-border rounded-xl p-6">
            <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted mb-3">
              Reward History
            </p>
            <p className="text-sm text-fry-text-muted italic">
              Historical reward data is not yet available.
            </p>
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
