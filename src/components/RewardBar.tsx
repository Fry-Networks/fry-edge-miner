interface RewardBarProps {
  active: number
  total: number
  proportion: number
}

export function RewardBar({ active, total, proportion }: RewardBarProps) {
  const percentage = Math.round(proportion * 100)

  return (
    <div className="space-y-3">
      {/* Header */}
      <div className="flex items-center justify-between">
        <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted">
          Slot Coverage
        </p>
        <span className="text-lg font-bold tabular-nums text-fry-neon">
          {percentage}%
        </span>
      </div>

      {/* Bar visualization */}
      <div className="flex gap-0.5">
        {Array.from({ length: Math.max(total, 1) }).map((_, idx) => (
          <div
            key={idx}
            className={`flex-1 h-2 rounded-full ${
              idx < active
                ? 'bg-gradient-to-r from-fry-neon to-fry-neon-dim'
                : 'bg-fry-border'
            }`}
          />
        ))}
      </div>

      {/* Label */}
      <div className="flex justify-between items-center">
        <span className="text-xs text-fry-text-muted">
          {active}/{total} active
        </span>
      </div>
    </div>
  )
}
