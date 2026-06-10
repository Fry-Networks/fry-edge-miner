import type { IntegrationStatus } from '../lib/types'
import { StatusIndicator } from './StatusIndicator'

interface IntegrationCardProps {
  integration: IntegrationStatus
  onToggle: (id: string, enabled: boolean) => void
}

export function IntegrationCard({
  integration,
  onToggle,
}: IntegrationCardProps) {
  return (
    <div className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-5 space-y-4">
      {/* Header with name and status */}
      <div className="flex items-start justify-between">
        <div>
          <h3 className="text-lg font-semibold text-fry-text">
            {integration.display_name}
          </h3>
        </div>
        <StatusIndicator status={integration.health} showLabel={false} />
      </div>

      {/* Health and lifecycle info */}
      <div className="space-y-2 border-t border-fry-border/60 pt-4">
        <div className="flex items-center justify-between text-sm">
          <span className="text-fry-text-muted">Health</span>
          <StatusIndicator status={integration.health} showLabel />
        </div>
        <div className="flex items-center justify-between text-sm">
          <span className="text-fry-text-muted">Lifecycle</span>
          <StatusIndicator status={integration.lifecycle} showLabel />
        </div>
        {integration.version && (
          <div className="flex items-center justify-between text-sm">
            <span className="text-fry-text-muted">Version</span>
            <span className="text-fry-text font-mono text-xs">
              {integration.version}
            </span>
          </div>
        )}
        <div className="flex items-center justify-between text-sm">
          <span className="text-fry-text-muted">PoC Contribution</span>
          <span className="text-fry-neon font-medium">
            {integration.poc_contribution.toFixed(1)}%
          </span>
        </div>
      </div>

      {/* Toggle button */}
      <button
        onClick={() => onToggle(integration.id, !integration.enabled)}
        className={`w-full px-4 py-2 rounded-lg font-medium transition ${
          integration.enabled
            ? 'bg-fry-red/20 text-fry-red hover:bg-fry-red/30 border border-fry-red/50'
            : 'bg-fry-surface-hover/60 text-fry-text-muted hover:bg-fry-surface-hover/80'
        }`}
      >
        {integration.enabled ? 'Disable' : 'Enable'}
      </button>
    </div>
  )
}
