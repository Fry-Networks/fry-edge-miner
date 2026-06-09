import { useIntegrations } from '../hooks/useIntegrations'
import { IntegrationCard } from '../components/IntegrationCard'

export default function Integrations() {
  const { integrations, loading, toggle } = useIntegrations()

  return (
    <div className="p-8 space-y-8">
      {/* Page Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-4xl font-bold text-white mb-2">Integrations</h1>
          <p className="text-gray-400">
            Enable or disable earning integrations
          </p>
        </div>
        <div className="bg-emerald-500/20 border border-emerald-500/50 rounded-lg px-4 py-2">
          <p className="text-sm font-medium text-emerald-400">
            {integrations.filter((i) => i.enabled).length}/{integrations.length} Active
          </p>
        </div>
      </div>

      {/* Integration Grid */}
      {loading ? (
        <div className="grid grid-cols-1 md:grid-cols-5 gap-4">
          {Array.from({ length: 5 }).map((_, idx) => (
            <div
              key={idx}
              className="bg-gray-900/80 border border-gray-800/60 rounded-xl p-5 animate-pulse"
            >
              <div className="h-6 bg-gray-800 rounded mb-4" />
              <div className="h-32 bg-gray-800 rounded" />
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
  )
}
