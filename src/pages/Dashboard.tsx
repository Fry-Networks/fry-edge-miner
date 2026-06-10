import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { Link } from 'react-router-dom'
import { useDevice } from '../hooks/useDevice'
import { useIntegrations } from '../hooks/useIntegrations'
import { useRewards } from '../hooks/useRewards'
import { IntegrationCard } from '../components/IntegrationCard'
import { RewardBar } from '../components/RewardBar'

export default function Dashboard() {
  const { device, loading: deviceLoading } = useDevice()
  const {
    integrations,
    loading: intLoading,
    toggle,
  } = useIntegrations()
  const { rewards, loading: rewardsLoading } = useRewards()

  const [hasMigration, setHasMigration] = useState(false)

  useEffect(() => {
    invoke('check_migration').then((info) => {
      if (info) setHasMigration(true)
    }).catch(() => {})
  }, [])

  const isLoading = deviceLoading || intLoading || rewardsLoading

  return (
    <div className="p-8 space-y-8">
      {/* Page Header */}
      <div>
        <h1 className="text-4xl font-bold text-fry-text mb-2">
          Fry Edge Miner
        </h1>
        <p className="text-fry-text-muted">
          Multi-integration decentralized platform client
        </p>
      </div>

      {/* Migration Banner */}
      {hasMigration && (
        <div className="bg-fry-info/10 border border-fry-info/40 rounded-xl p-5 flex items-center justify-between">
          <div>
            <p className="text-fry-info font-medium">FryHub Installation Detected</p>
            <p className="text-sm text-fry-info/70 mt-1">Migrate your existing miners to FEM for consolidated rewards.</p>
          </div>
          <Link
            to="/migration"
            className="px-5 py-2 bg-fry-info/60 hover:bg-fry-info/50 rounded-lg font-medium transition-colors text-sm"
          >
            Migrate Now
          </Link>
        </div>
      )}

      {/* Device Info Section */}
      {!device?.registered ? (
        <div className="bg-fry-warning/20 border border-fry-warning/50 rounded-xl p-6">
          <p className="text-fry-warning font-medium mb-2">
            Device Not Registered
          </p>
          <p className="text-sm text-fry-warning/90">
            Complete device setup in Settings to begin mining.
          </p>
        </div>
      ) : (
        <div className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-6 space-y-4">
          <h2 className="text-xl font-semibold text-fry-text">Device Info</h2>
          <div className="grid grid-cols-2 gap-6">
            <div>
              <p className="text-sm text-fry-text-muted mb-1">Miner Key</p>
              <p className="font-mono text-sm text-fry-text break-all">
                {device.miner_key ? device.miner_key.slice(0, 20) + '...' : '-'}
              </p>
            </div>
            <div>
              <p className="text-sm text-fry-text-muted mb-1">Wallet Address</p>
              <p className="font-mono text-sm text-fry-text break-all">
                {device.wallet_address
                  ? device.wallet_address.slice(0, 20) + '...'
                  : '-'}
              </p>
            </div>
          </div>
        </div>
      )}

      {/* Integrations Grid */}
      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <h2 className="text-xl font-semibold text-fry-text">Integrations</h2>
          <span className="text-sm text-fry-text-muted">
            {integrations.filter((i) => i.enabled).length}/{integrations.length}{' '}
            active
          </span>
        </div>
        {isLoading ? (
          <div className="grid grid-cols-5 gap-4">
            {Array.from({ length: 5 }).map((_, idx) => (
              <div
                key={idx}
                className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-5 animate-pulse"
              >
                <div className="h-6 bg-fry-border rounded mb-4" />
                <div className="h-20 bg-fry-border rounded" />
              </div>
            ))}
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-5 gap-4">
            {integrations.map((integration) => (
              <IntegrationCard
                key={integration.id}
                integration={integration}
                onToggle={toggle}
              />
            ))}
          </div>
        )}
      </div>

      {/* Reward Summary */}
      {rewards && (
        <div className="space-y-4">
          <h2 className="text-xl font-semibold text-fry-text">
            Reward Summary
          </h2>
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
            <RewardBar
              active={rewards.summary.active_count}
              total={rewards.summary.total_count}
              proportion={rewards.summary.proportion}
            />
          </div>
        </div>
      )}
    </div>
  )
}
