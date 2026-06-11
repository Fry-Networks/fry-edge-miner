import type { IntegrationStatus } from '../lib/types'
import { StatusIndicator } from './StatusIndicator'
import { Toggle } from './Toggle'

interface IntegrationCardProps {
  integration: IntegrationStatus
  onToggle: (id: string, enabled: boolean) => void
}

export function IntegrationCard({
  integration,
  onToggle,
}: IntegrationCardProps) {
  const isHealthy = integration.health === 'Healthy'

  // Left accent border based on state
  const accentClass = integration.enabled
    ? isHealthy
      ? 'border-l-2 border-l-fry-neon'
      : 'border-l-2 border-l-fry-warning'
    : ''

  return (
    <div className={`bg-fry-surface border border-fry-border rounded-xl p-5 space-y-4 ${accentClass}`}>
      {/* Header: status dot + name + toggle */}
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2 min-w-0">
          <StatusIndicator status={integration.health} showLabel={false} />
          <h3 className="text-sm font-medium text-fry-text truncate">
            {integration.display_name}
          </h3>
        </div>
        <Toggle
          checked={integration.enabled}
          onChange={() => onToggle(integration.id, !integration.enabled)}
        />
      </div>

      {/* Details */}
      <div className="space-y-2 border-t border-fry-border-subtle pt-3">
        <div className="flex items-center justify-between">
          <span className="text-xs text-fry-text-muted">Health</span>
          <StatusIndicator status={integration.health} showLabel />
        </div>
        <div className="flex items-center justify-between">
          <span className="text-xs text-fry-text-muted">Lifecycle</span>
          <StatusIndicator status={integration.lifecycle} showLabel />
        </div>
        {integration.version && (
          <div className="flex items-center justify-between">
            <span className="text-xs text-fry-text-muted">Version</span>
            <span className="font-mono text-xs text-fry-text-muted">
              {integration.version}
            </span>
          </div>
        )}
        <div className="flex items-center justify-between">
          <span className="text-xs text-fry-text-muted">PoC</span>
          <span className="text-fry-neon font-medium text-sm">
            {integration.poc_contribution.toFixed(1)}%
          </span>
        </div>
      </div>
    </div>
  )
}
