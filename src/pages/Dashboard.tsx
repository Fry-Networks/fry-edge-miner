import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { Link } from 'react-router-dom'
import { useDevice } from '../hooks/useDevice'
import { useIntegrations } from '../hooks/useIntegrations'
import { useRewards } from '../hooks/useRewards'
import { IntegrationCard } from '../components/IntegrationCard'
import { RewardBar } from '../components/RewardBar'
import { PageHeader } from '../components/PageHeader'

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
    <div className="p-6 lg:p-8 space-y-6">
      <PageHeader title="Dashboard" subtitle="System overview" />

      {/* Migration Banner */}
      {hasMigration && (
        <div className="bg-fry-surface border-l-4 border-l-fry-info border border-fry-border rounded-lg p-5 flex items-center justify-between">
          <div>
            <p className="text-fry-info font-medium text-sm">FryHub Installation Detected</p>
            <p className="text-xs text-fry-text-muted mt-1">Migrate your existing miners to FEM for consolidated rewards.</p>
          </div>
          <Link
            to="/migration"
            className="px-4 py-1.5 text-xs font-medium bg-fry-info text-white rounded-md hover:opacity-90 transition-opacity"
          >
            Migrate Now
          </Link>
        </div>
      )}

      {/* Device Info Section */}
      {!device?.registered ? (
        <div className="bg-fry-surface border-l-4 border-l-fry-warning border border-fry-border rounded-lg p-5">
          <p className="text-fry-warning font-medium text-sm mb-1">
            Device Not Registered
          </p>
          <p className="text-xs text-fry-text-muted">
            Complete device setup in Settings to begin mining.{' '}
            <Link to="/settings" className="text-fry-warning underline">
              Configure now
            </Link>
          </p>
        </div>
      ) : (
        <div className="bg-fry-surface border border-fry-border rounded-xl p-6 space-y-4">
          <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted">Device Info</p>
          <div className="grid grid-cols-2 gap-6">
            <div>
              <p className="text-xs text-fry-text-muted mb-1">Miner Key</p>
              <code className="font-mono text-xs bg-fry-surface-2 px-2 py-0.5 rounded text-fry-text-muted block truncate">
                {device.miner_key ? device.miner_key.slice(0, 20) + '...' : '\u2014'}
              </code>
            </div>
            <div>
              <p className="text-xs text-fry-text-muted mb-1">Wallet Address</p>
              <code className="font-mono text-xs bg-fry-surface-2 px-2 py-0.5 rounded text-fry-text-muted block truncate">
                {device.wallet_address
                  ? device.wallet_address.slice(0, 20) + '...'
                  : '\u2014'}
              </code>
            </div>
          </div>
        </div>
      )}

      {/* Integrations Grid */}
      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted">Integrations</p>
          <span className="text-xs text-fry-text-muted">
            <span className="text-fry-neon font-medium">{integrations.filter((i) => i.enabled).length}</span>/{integrations.length} active
          </span>
        </div>
        {isLoading ? (
          <div className="grid grid-cols-5 gap-4">
            {Array.from({ length: 5 }).map((_, idx) => (
              <div
                key={idx}
                className="bg-fry-surface border border-fry-border rounded-xl p-5 animate-pulse"
              >
                <div className="h-5 bg-fry-border-subtle rounded mb-3" />
                <div className="h-16 bg-fry-border-subtle rounded" />
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
          <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted">
            Reward Summary
          </p>
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
