import type { HealthStatus, LifecycleState } from '../lib/types'
import { getHealthLabel } from '../lib/types'

interface StatusIndicatorProps {
  status: HealthStatus | LifecycleState
  showLabel?: boolean
}

export function StatusIndicator({ status, showLabel = true }: StatusIndicatorProps) {
  let colorClass = 'bg-fry-text-muted'
  let glowColor: string | null = null
  let label = 'Unknown'
  let pulse = false

  if (typeof status === 'string') {
    switch (status) {
      case 'Healthy':
      case 'Running':
        colorClass = 'bg-fry-neon'
        glowColor = '#00B69B'
        label = status
        pulse = true
        break
      case 'Starting':
      case 'Installing':
      case 'Updating':
      case 'Restarting':
        colorClass = 'bg-fry-warning'
        glowColor = '#f97316'
        label = status
        pulse = true
        break
      case 'Unhealthy':
      case 'Failed':
        colorClass = 'bg-fry-error'
        label = status
        break
      case 'Disabled':
      case 'Stopped':
        colorClass = 'bg-fry-text-muted'
        label = status
        break
      default:
        colorClass = 'bg-fry-text-muted'
        label = status
    }
  } else if ('Unhealthy' in status) {
    colorClass = 'bg-fry-error'
    label = getHealthLabel(status)
  }

  const dotStyle = pulse && glowColor
    ? { boxShadow: `0 0 6px ${glowColor}`, animation: 'status-pulse 2s ease-in-out infinite' }
    : undefined

  return (
    <span className="flex items-center gap-2">
      <span className={`h-2.5 w-2.5 rounded-full shrink-0 ${colorClass}`} style={dotStyle} />
      {showLabel && <span className="text-xs text-fry-text-muted">{label}</span>}
    </span>
  )
}
