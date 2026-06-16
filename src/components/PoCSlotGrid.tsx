import { useState } from 'react'
import type { PocSlot } from '../lib/types'

interface PoCSlotGridProps {
  slots: PocSlot[]
}

const ROWS = 24
const COLS = 6
const GRID_SIZE = ROWS * COLS // 144

export function PoCSlotGrid({ slots }: PoCSlotGridProps) {
  const [hoveredSlot, setHoveredSlot] = useState<number | null>(null)

  const slotMap = new Map(slots.map((s) => [s.slot_index, s]))

  const allPassCount = slots.filter(
    (s) => s.data && s.online && s.mac_match && s.pol && s.poi && s.poa
  ).length

  const cells = Array.from({ length: GRID_SIZE }).map((_, idx) => {
    const slot = slotMap.get(idx)
    if (!slot) return { index: idx, color: 'bg-fry-border', slot: null }
    const allPass = slot.data && slot.online && slot.mac_match && slot.pol && slot.poi && slot.poa
    if (allPass) return { index: idx, color: 'bg-fry-neon', slot }
    if (slot.data || slot.online || slot.mac_match || slot.pol || slot.poi || slot.poa) {
      return { index: idx, color: 'bg-fry-warning', slot }
    }
    return { index: idx, color: 'bg-fry-error', slot }
  })

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <span className="text-xs text-fry-text-muted">
          {allPassCount}/{slots.length} slots passing all gates
        </span>
        <div className="flex gap-3 text-xs text-fry-text-muted">
          {[
            { color: 'bg-fry-neon', label: 'Pass' },
            { color: 'bg-fry-warning', label: 'Partial' },
            { color: 'bg-fry-error', label: 'Fail' },
            { color: 'bg-fry-border', label: 'Empty' },
          ].map(({ color, label }) => (
            <div key={label} className="flex items-center gap-1.5">
              <div className={`w-2.5 h-2.5 rounded-sm ${color}`} />
              <span>{label}</span>
            </div>
          ))}
        </div>
      </div>

      <div className="space-y-[2px]">
        {Array.from({ length: ROWS }).map((_, rowIdx) => {
          const hourLabel = `${String(rowIdx).padStart(2, '0')}:00`
          const rowCells = cells.slice(rowIdx * COLS, rowIdx * COLS + COLS)
          return (
            <div key={rowIdx} className="flex items-center gap-2">
              <span className="text-[10px] text-fry-text-muted w-9 shrink-0 text-right tabular-nums">
                {hourLabel}
              </span>
              <div className="grid grid-cols-6 gap-[2px] flex-1">
                {rowCells.map((cell) => (
                  <div
                    key={cell.index}
                    className="relative"
                    onMouseEnter={() => setHoveredSlot(cell.index)}
                    onMouseLeave={() => setHoveredSlot(null)}
                  >
                    <div
                      className={`h-3 w-full rounded-sm ${cell.color} opacity-80 hover:opacity-100 transition-opacity cursor-default`}
                      title={cell.slot ? `Slot ${cell.index}: ${cell.slot.tools_count} tools` : 'No data'}
                    />
                    {hoveredSlot === cell.index && cell.slot && (
                      <div
                        className={`absolute left-1/2 -translate-x-1/2 bg-fry-bg border border-fry-border rounded px-3 py-2 text-xs text-fry-text whitespace-nowrap z-50 ${
                          rowIdx < 4 ? 'top-full mt-1' : 'bottom-full mb-1'
                        }`}
                      >
                        <div className="font-semibold">Slot {cell.index}</div>
                        <div className="text-fry-text-muted">{cell.slot.tools_count} tools</div>
                        <div className="text-fry-text-muted">×{cell.slot.multiplier.toFixed(2)}</div>
                      </div>
                    )}
                  </div>
                ))}
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}
