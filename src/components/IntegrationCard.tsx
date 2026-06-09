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
    <div className="bg-gray-900/80 border border-gray-800/60 rounded-xl p-5 space-y-4">
      {/* Header with name and status */}
      <div className="flex items-start justify-between">
        <div>
          <h3 className="text-lg font-semibold text-white">
            {integration.display_name}
          </h3>
        </div>
        <StatusIndicator status={integration.health} showLabel={false} />
      </div>

      {/* Health and lifecycle info */}
      <div className="space-y-2 border-t border-gray-800/60 pt-4">
        <div className="flex items-center justify-between text-sm">
          <span className="text-gray-500">Health</span>
          <StatusIndicator status={integration.health} showLabel />
        </div>
        <div className="flex items-center justify-between text-sm">
          <span className="text-gray-500">Lifecycle</span>
          <StatusIndicator status={integration.lifecycle} showLabel />
        </div>
        {integration.version && (
          <div className="flex items-center justify-between text-sm">
            <span className="text-gray-500">Version</span>
            <span className="text-gray-300 font-mono text-xs">
              {integration.version}
            </span>
          </div>
        )}
        <div className="flex items-center justify-between text-sm">
          <span className="text-gray-500">PoC Contribution</span>
          <span className="text-emerald-400 font-medium">
            {integration.poc_contribution.toFixed(1)}%
          </span>
        </div>
      </div>

      {/* Toggle button */}
      <button
        onClick={() => onToggle(integration.id, !integration.enabled)}
        className={`w-full px-4 py-2 rounded-lg font-medium transition ${
          integration.enabled
            ? 'bg-emerald-500/20 text-emerald-400 hover:bg-emerald-500/30'
            : 'bg-gray-800/60 text-gray-400 hover:bg-gray-800/80'
        }`}
      >
        {integration.enabled ? 'Disable' : 'Enable'}
      </button>
    </div>
  )
}
