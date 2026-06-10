import { useRewards } from '../hooks/useRewards'
import { RewardBar } from '../components/RewardBar'
import { PoCSlotGrid } from '../components/PoCSlotGrid'

export default function Rewards() {
  const { rewards, loading } = useRewards()

  return (
    <div className="p-8 space-y-8">
      {/* Page Header */}
      <div>
        <h1 className="text-4xl font-bold text-fry-text mb-2">
          Proportional Rewards
        </h1>
        <p className="text-fry-text-muted">
          Earnings distribution across active integrations
        </p>
      </div>

      {loading ? (
        <div className="space-y-6">
          {Array.from({ length: 3 }).map((_, idx) => (
            <div
              key={idx}
              className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-6 animate-pulse"
            >
              <div className="h-6 bg-fry-border rounded" />
            </div>
          ))}
        </div>
      ) : rewards ? (
        <>
          {/* Summary Cards */}
          <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
            <div className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-6">
              <p className="text-sm text-fry-text-muted mb-2">Active Rewards</p>
              <p className="text-3xl font-bold text-fry-neon">
                {rewards.summary.active_count}
              </p>
            </div>
            <div className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-6">
              <p className="text-sm text-fry-text-muted mb-2">Proportion</p>
              <p className="text-3xl font-bold text-fry-neon">
                {Math.round(rewards.summary.proportion * 100)}%
              </p>
            </div>
            <div className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-6">
              <p className="text-sm text-fry-text-muted mb-2">Est. Daily</p>
              <p className="text-3xl font-bold text-fry-neon">
                ${rewards.summary.estimated_daily.toFixed(2)}
              </p>
            </div>
          </div>

          {/* Reward Bar */}
          <div className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-6">
            <h2 className="text-lg font-semibold text-fry-text mb-4">
              Reward Distribution
            </h2>
            <RewardBar
              active={rewards.summary.active_count}
              total={rewards.summary.total_count}
              proportion={rewards.summary.proportion}
            />
          </div>

          {/* PoC Slot Grid */}
          <div className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-6">
            <h2 className="text-lg font-semibold text-fry-text mb-4">
              PoC Slot Status
            </h2>
            <PoCSlotGrid slots={rewards.slots} />
          </div>

          {/* Reward History Placeholder */}
          <div className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-6">
            <h2 className="text-lg font-semibold text-fry-text mb-2">
              Reward History
            </h2>
            <p className="text-fry-text-muted text-sm">
              Reward history coming soon
            </p>
          </div>
        </>
      ) : null}
    </div>
  )
}
