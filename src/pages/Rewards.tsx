import { useRewards } from '../hooks/useRewards'
import { RewardBar } from '../components/RewardBar'
import { PoCSlotGrid } from '../components/PoCSlotGrid'

export default function Rewards() {
  const { rewards, loading } = useRewards()

  return (
    <div className="p-8 space-y-8">
      {/* Page Header */}
      <div>
        <h1 className="text-4xl font-bold text-white mb-2">
          Proportional Rewards
        </h1>
        <p className="text-gray-400">
          Earnings distribution across active integrations
        </p>
      </div>

      {loading ? (
        <div className="space-y-6">
          {Array.from({ length: 3 }).map((_, idx) => (
            <div
              key={idx}
              className="bg-gray-900/80 border border-gray-800/60 rounded-xl p-6 animate-pulse"
            >
              <div className="h-6 bg-gray-800 rounded" />
            </div>
          ))}
        </div>
      ) : rewards ? (
        <>
          {/* Summary Cards */}
          <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
            <div className="bg-gray-900/80 border border-gray-800/60 rounded-xl p-6">
              <p className="text-sm text-gray-500 mb-2">Active Rewards</p>
              <p className="text-3xl font-bold text-emerald-400">
                {rewards.summary.active_count}
              </p>
            </div>
            <div className="bg-gray-900/80 border border-gray-800/60 rounded-xl p-6">
              <p className="text-sm text-gray-500 mb-2">Proportion</p>
              <p className="text-3xl font-bold text-emerald-400">
                {Math.round(rewards.summary.proportion * 100)}%
              </p>
            </div>
            <div className="bg-gray-900/80 border border-gray-800/60 rounded-xl p-6">
              <p className="text-sm text-gray-500 mb-2">Est. Daily</p>
              <p className="text-3xl font-bold text-emerald-400">
                ${rewards.summary.estimated_daily.toFixed(2)}
              </p>
            </div>
          </div>

          {/* Reward Bar */}
          <div className="bg-gray-900/80 border border-gray-800/60 rounded-xl p-6">
            <h2 className="text-lg font-semibold text-white mb-4">
              Reward Distribution
            </h2>
            <RewardBar
              active={rewards.summary.active_count}
              total={rewards.summary.total_count}
              proportion={rewards.summary.proportion}
            />
          </div>

          {/* PoC Slot Grid */}
          <div className="bg-gray-900/80 border border-gray-800/60 rounded-xl p-6">
            <h2 className="text-lg font-semibold text-white mb-4">
              PoC Slot Status
            </h2>
            <PoCSlotGrid slots={rewards.slots} />
          </div>

          {/* Reward History Placeholder */}
          <div className="bg-gray-900/80 border border-gray-800/60 rounded-xl p-6">
            <h2 className="text-lg font-semibold text-white mb-2">
              Reward History
            </h2>
            <p className="text-gray-400 text-sm">
              Reward history coming soon
            </p>
          </div>
        </>
      ) : null}
    </div>
  )
}
