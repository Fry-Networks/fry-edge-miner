import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { Link } from 'react-router-dom'
import { Activity, Percent, DollarSign } from 'lucide-react'
import { useIntegrations } from '../hooks/useIntegrations'
import { useRewards } from '../hooks/useRewards'
import { IntegrationCardMini } from '../components/IntegrationCard'
import { RewardBar } from '../components/RewardBar'
import { StatCard } from '../components/StatCard'
import { PageHeader } from '../components/PageHeader'

export default function Dashboard() {
  const { integrations, loading: intLoading, toggle } = useIntegrations()
  const { rewards, loading: rewardsLoading } = useRewards()
  const [hasMigration, setHasMigration] = useState(false)

  useEffect(() => {
    invoke('check_migration').then((info) => {
      if (info) setHasMigration(true)
    }).catch(() => {})
  }, [])

  const isLoading = intLoading || rewardsLoading

  return (
    <div className="p-6 lg:p-8 space-y-6">
      <PageHeader title="Dashboard" subtitle="System overview" />

      {/* Migration banner */}
      {hasMigration && (
        <div className="bg-fry-surface border-l-4 border-l-fry-info border border-fry-border rounded-lg p-5 flex items-center justify-between">
          <div>
            <p className="text-fry-info font-medium text-sm">FryHub Installation Detected</p>
            <p className="text-xs text-fry-text-muted mt-1">Migrate your existing miners to FEM for consolidated rewards.</p>
          </div>
          <Link
            to="/migration"
            className="px-4 py-1.5 text-xs font-medium bg-fry-info text-white rounded-md hover:opacity-90 transition-opacity shrink-0"
          >
            Migrate Now
          </Link>
        </div>
      )}

      {/* Stat cards */}
      {rewards && (
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
      )}

      {/* Integrations */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted">Integrations</p>
          <span className="text-xs text-fry-text-muted">
            <span className="text-fry-neon font-medium">{integrations.filter((i) => i.enabled).length}</span>/{integrations.length} active
          </span>
        </div>
        {isLoading ? (
          <div className="grid grid-cols-5 gap-3">
            {Array.from({ length: 5 }).map((_, idx) => (
              <div key={idx} className="bg-fry-surface border border-fry-border rounded-xl p-4 animate-pulse h-16" />
            ))}
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-5 gap-3">
            {integrations.map((integration) => (
              <IntegrationCardMini
                key={integration.id}
                integration={integration}
                onToggle={toggle}
              />
            ))}
          </div>
        )}
      </div>

      {/* PoC Distribution */}
      {rewards && (
        <div className="bg-fry-surface border border-fry-border rounded-xl p-6">
          <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted mb-4">
            PoC Distribution
          </p>
          <RewardBar
            active={rewards.summary.active_count}
            total={rewards.summary.total_count}
            proportion={rewards.summary.proportion}
          />
        </div>
      )}
    </div>
  )
}
