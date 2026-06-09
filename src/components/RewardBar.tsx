interface RewardBarProps {
  active: number
  total: number
  proportion: number
}

export function RewardBar({ active, total, proportion }: RewardBarProps) {
  const percentage = Math.round(proportion * 100)

  return (
    <div className="space-y-3">
      {/* Bar visualization */}
      <div className="flex gap-1">
        {Array.from({ length: Math.max(total, 1) }).map((_, idx) => (
          <div
            key={idx}
            className={`flex-1 h-3 rounded ${
              idx < active ? 'bg-emerald-500' : 'bg-gray-700'
            }`}
          />
        ))}
      </div>

      {/* Label */}
      <div className="flex justify-between items-center">
        <span className="text-sm text-gray-400">
          {active}/{total} active
        </span>
        <span className="text-sm font-semibold text-emerald-400">
          {percentage}%
        </span>
      </div>
    </div>
  )
}
