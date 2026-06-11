import type { HealthStatus, LifecycleState } from '../lib/types'
import { getHealthLabel } from '../lib/types'

interface StatusIndicatorProps {
  status: HealthStatus | LifecycleState
  showLabel?: boolean
}

export function StatusIndicator({
  status,
  showLabel = true,
}: StatusIndicatorProps) {
  let bgColor = 'bg-fry-text-muted'
  let label = 'Unknown'
  let pulse = false

  // Determine color and label based on status
  if (typeof status === 'string') {
    switch (status) {
      // Healthy/Running states
      case 'Healthy':
      case 'Running':
        bgColor = 'bg-fry-neon'
        label = status
        pulse = true
        break
      // Warning/Transitional states
      case 'Starting':
      case 'Installing':
      case 'Updating':
      case 'Restarting':
        bgColor = 'bg-fry-warning'
        label = status
        pulse = true
        break
      // Error/Offline states
      case 'Unhealthy':
      case 'Failed':
        bgColor = 'bg-fry-error'
        label = status
        break
      // Disabled/Stopped states
      case 'Disabled':
      case 'Stopped':
        bgColor = 'bg-fry-text-muted'
        label = status
        break
      default:
        bgColor = 'bg-fry-text-muted'
        label = status
    }
  } else if ('Unhealthy' in status) {
    bgColor = 'bg-fry-error'
    label = getHealthLabel(status)
  }

  return (
    <span className="flex items-center gap-2">
      <span className={`h-2 w-2 rounded-full ${bgColor} ${pulse ? 'animate-pulse' : ''}`} />
      {showLabel && <span className="text-xs text-fry-text-muted">{label}</span>}
    </span>
  )
}
