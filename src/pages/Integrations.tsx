import { useIntegrations } from '../hooks/useIntegrations'
import { IntegrationCard } from '../components/IntegrationCard'
import { PageHeader } from '../components/PageHeader'

export default function Integrations() {
  const { integrations, loading, toggle } = useIntegrations()

  const enabledCount = integrations.filter((i) => i.enabled).length

  return (
    <div className="p-6 lg:p-8 space-y-6">
      <PageHeader
        title="Integrations"
        subtitle="Enable or disable earning integrations"
        action={
          <span className="bg-fry-surface border border-fry-border text-fry-text-muted text-xs px-3 py-1 rounded-full">
            <span className="text-fry-neon font-medium">{enabledCount}</span>/{integrations.length} active
          </span>
        }
      />

      {/* Integration Grid */}
      {loading ? (
        <div className="grid grid-cols-1 md:grid-cols-5 gap-4">
          {Array.from({ length: 5 }).map((_, idx) => (
            <div
              key={idx}
              className="bg-fry-surface border border-fry-border rounded-xl p-5 animate-pulse"
            >
              <div className="flex items-center justify-between mb-3">
                <div className="h-4 w-20 bg-fry-border-subtle rounded" />
                <div className="h-5 w-9 bg-fry-border-subtle rounded-full" />
              </div>
              <div className="space-y-2 border-t border-fry-border-subtle pt-3">
                <div className="h-3 bg-fry-border-subtle rounded" />
                <div className="h-3 bg-fry-border-subtle rounded" />
                <div className="h-3 bg-fry-border-subtle rounded" />
              </div>
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
