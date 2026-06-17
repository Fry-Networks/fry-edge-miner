import { useNavigate } from 'react-router-dom'
import type { IntegrationStatus } from '../lib/types'
import { StatusIndicator } from './StatusIndicator'
import { Toggle } from './Toggle'

interface IntegrationCardProps {
  integration: IntegrationStatus
  onToggle: (id: string, enabled: boolean) => void
}

/** Compact card for Dashboard 5-col grid */
export function IntegrationCardMini({ integration, onToggle }: IntegrationCardProps) {
  const notInstalled = integration.version === null

  return (
    <div className={`bg-fry-surface border border-fry-border rounded-xl p-4 space-y-2 ${notInstalled ? 'opacity-60' : ''}`}>
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-1.5 min-w-0">
          <StatusIndicator status={integration.health} showLabel={false} />
          <span className="text-xs font-medium text-fry-text truncate">{integration.display_name}</span>
        </div>
        <Toggle
          checked={integration.enabled}
          onChange={() => onToggle(integration.id, !integration.enabled)}
          disabled={notInstalled}
        />
      </div>
    </div>
  )
}

/** Full card for Integrations page */
export function IntegrationCard({ integration, onToggle }: IntegrationCardProps) {
  const navigate = useNavigate()
  const notInstalled = integration.version === null
  const isHealthy = integration.health === 'Healthy'

  const accentClass = !notInstalled && integration.enabled
    ? isHealthy
      ? 'border-l-2 border-l-fry-neon'
      : 'border-l-2 border-l-fry-warning'
    : ''

  return (
    <div className={`bg-fry-surface border border-fry-border rounded-xl p-5 space-y-4 ${accentClass} ${notInstalled ? 'opacity-70' : ''}`}>
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2 min-w-0">
          <StatusIndicator status={integration.health} showLabel={false} />
          <h3 className="text-sm font-medium text-fry-text truncate">{integration.display_name}</h3>
        </div>
        {notInstalled ? (
          <span className="text-[10px] bg-fry-border text-fry-text-muted px-1.5 py-0.5 rounded shrink-0">
            Not installed
          </span>
        ) : (
          <Toggle
            checked={integration.enabled}
            onChange={() => onToggle(integration.id, !integration.enabled)}
          />
        )}
      </div>

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
            <span className="font-mono text-xs text-fry-text-muted">{integration.version}</span>
          </div>
        )}
        <div className="flex items-center justify-between">
          <span className="text-xs text-fry-text-muted">PoC</span>
          <span className="text-fry-neon font-medium text-sm">{(integration.poc_contribution * 100).toFixed(1)}%</span>
        </div>
        {notInstalled && (
          <div className="pt-2">
            <button
              onClick={() => navigate('/updates')}
              className="text-xs text-fry-neon hover:text-fry-neon-dim transition-colors"
            >
              Install from Updates &rarr;
            </button>
          </div>
        )}
      </div>
    </div>
  )
}
