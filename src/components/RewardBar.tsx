interface RewardBarProps {
  active: number
  total: number
  proportion: number
}

export function RewardBar({ active, total, proportion }: RewardBarProps) {
  const percentage = Math.round(proportion * 100)
  const fillWidth = Math.min(100, Math.max(0, proportion * 100))

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted">
          Slot Coverage
        </p>
        <span className="text-lg font-bold tabular-nums text-fry-neon">{percentage}%</span>
      </div>
      <div className="h-2 rounded-full bg-fry-border overflow-hidden">
        <div
          className="h-full rounded-full transition-all duration-500"
          style={{ width: `${fillWidth}%`, background: 'linear-gradient(90deg, #00B69B, #007d6c)' }}
        />
      </div>
      <div className="flex justify-between items-center">
        <span className="text-xs text-fry-text-muted">{active}/{total} active</span>
      </div>
    </div>
  )
}
