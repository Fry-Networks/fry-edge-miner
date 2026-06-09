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
  let bgColor = 'bg-gray-500'
  let label = 'Unknown'

  // Determine color and label based on status
  if (typeof status === 'string') {
    switch (status) {
      // Healthy/Running states
      case 'Healthy':
      case 'Running':
        bgColor = 'bg-emerald-500'
        label = status
        break
      // Warning/Transitional states
      case 'Starting':
      case 'Installing':
      case 'Updating':
      case 'Restarting':
        bgColor = 'bg-amber-500'
        label = status
        break
      // Error/Offline states
      case 'Unhealthy':
      case 'Failed':
        bgColor = 'bg-red-500'
        label = status
        break
      // Disabled/Stopped states
      case 'Disabled':
      case 'Stopped':
        bgColor = 'bg-gray-500'
        label = status
        break
      default:
        bgColor = 'bg-gray-500'
        label = status
    }
  } else if ('Unhealthy' in status) {
    bgColor = 'bg-red-500'
    label = getHealthLabel(status)
  }

  return (
    <span className="flex items-center gap-2">
      <span className={`h-2.5 w-2.5 rounded-full ${bgColor}`} />
      {showLabel && <span className="text-sm text-gray-300">{label}</span>}
    </span>
  )
}
